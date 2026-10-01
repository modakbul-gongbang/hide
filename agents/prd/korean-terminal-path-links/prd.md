---
topic: "한국어 문장 속 터미널 파일 링크"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "터미널 경로 해석과 실제 파일 선택 범위를 바꾸므로 링크 오인식과 호버 파일 검사 비용을 함께 검증한다."
source_intake: "current conversation"
created_at: "2026-10-01"
updated_at: "2026-10-01"
---

# PRD: 한국어 문장 속 터미널 파일 링크

## Goal

hide 사용자가 한국어 문장 속의 실제 로컬 파일 경로에 마우스를 올리고 클릭하면, 닫는 기호와 조사 때문에 링크가 사라지지 않고 정확한 파일을 기존 방식으로 열 수 있게 한다.
이슈 #271의 파일명 보존·링크 범위·기존 기능·성능 완료 조건을 모두 충족한다.

## Non-goals

- 존재하지 않는 경로를 추측해서 링크로 만들거나 조사처럼 보이는 실제 파일명을 무조건 줄이지 않는다. 실제 존재 확인이 링크의 근거다.
- 자연어 전체를 분석하는 새 모델·서비스·형태소 패키지를 넣지 않는다. 닫는 기호 뒤 문법 접미부라는 명시적인 경계만 해석한다.
- 입력·렌더링·snapshot 갱신 때 파일 검사를 추가하지 않는다. 기존 hover provider와 host의 일괄 검사 경계를 유지한다.
- 원격 단말의 파일을 새로 검사하거나 새로운 daemon 파일 API를 추가하지 않는다. remote/no-host에서 기존 링크 지원 범위를 유지한다.
- URL·OSC 8·파일 열기 보안·외부 파일 reveal 정책을 바꾸지 않는다. 이들은 기존 계약과 라우팅을 따른다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | `(docs/README.md)에`처럼 닫는 기호와 한국어 조사가 붙은 표현은 실제 경로만 링크로 선택한다. | 이슈 #271의 기대 동작과 첫 완료 조건. |
| D-02 | 조사·닫는 기호·위치처럼 보이는 suffix를 제거하기 전에 원래 경로 spelling의 존재를 확인한다. 여러 해석이 존재하면 가장 긴 실제 경로가 우선한다. | 이슈 #271의 실제 파일명 보존 완료 조건. |
| D-03 | 가정: 닫는 괄호·대괄호·중괄호·인용 부호 뒤의 에·에서·에게·께·으로·로·와·과·을·를·은·는·이·가·의·도·만·부터·까지 및 이들과 결합한 도·만·는·은 같은 유한한 문법 suffix를 지원한다. 임의 Hangul 단어를 반복 절단하지 않는다. | 사용자 추가 결정 위임. 첫 사례에 국한하지 않되 명시적 문법 경계와 유한 후보를 유지하기 위한 가정. |
| D-04 | 위임 가정 수정: 각 joined spelling은 원문과 기존 기호 정리 spelling, 각각에서 문법 접미부만 제거해 실제 닫는 기호와 앞쪽 기호를 보존한 spelling, 문법·기호를 정리하되 위치 문자열을 보존한 literal spelling, 최종 위치 해석이라는 유한한 최대 6개 후보를 제공한다. 동일 text/target은 중복 제거하며 임의 글자 반복 절단은 하지 않는다. 기존 최대 16개 joined spelling을 유지하여 토큰별 총 cwd/root lookup은 최대 192개, 원문 외 추가 lookup은 최대 160개다. | 독립 Fidelity·Code 리뷰가 괄호·조사·위치 결합에서 3개 상한과 D-02의 충돌을 재현했다. Spec Owner는 사용자의 추가 결정 위임으로 상한을 수정하며, 원문에서 조사만 제거한 실제 앞쪽·닫는 기호 포함 파일명도 보존한다. engineering 13·15. |
| D-05 | 가정: 한 hover 제공 호출의 새로운 고유 경로 lookup은 최대 512개로 제한하고 batch당 64개를 유지한다. budget은 원문 후보에 먼저 쓰며 초과한 추론 후보는 링크로 확정하지 않고 원인·수치를 진단한다. cache hit는 실제 새 파일 검사 예산을 소비하지 않는다. | 사용자 추가 결정 위임. cache 크기만으로 전체 파일 확인 횟수가 제한되지 않는다는 조사. |
| D-06 | 후보의 terminal range는 UTF-16 문자열 길이가 아니라 실제 cell 위치와 wrapped row를 따라 계산해 조사·기호를 제외한다. | 이슈 #271의 범위 정확성, wide Hangul과 wrapping을 다루는 기존 parser 계약. |
| D-07 | 기존 10초 TTL·512 cache 항목·dedup·64개 batching을 재사용하고 캐시와 native 검사 건수를 구분해 측정한다. 같은 의도를 재시도해도 같은 파일과 범위를 선택한다. | 기존 terminalLinkProvider 조사 및 engineering 7·11·15. |
| D-08 | 정확한 경로와 line/column 해석이 충돌하면 실제 원문 파일을 우선하고, 그렇지 않으면 기존 위치 해석을 유지한다. URL·OSC 8은 기존 처리를 따른다. | 이슈 #271의 원문 우선과 기존 기능 유지. |
| D-09 | 각 이슈별 worktree에서 PR·CI·merge commit을 완료하고 사용자가 사후 QA한다. 되돌릴 수 있는 추가 구현·제품 결정은 위임되었다. | 사용자 원문: “issue하나당 worktree 파서 /please 로 다 PR올리고 머지하는것까지”, “나대신 추가로 나오는 의사결정 다해주고”. agents/config.json의 PR·CI 감시·worktree 설정. |
| D-10 | engineering/principles.md와 design/principles.md 전체를 읽고 engineering 4·5·7·9·11·12·13·15 및 design 5·10·12·13을 적용한다. 새 리스트·폼·파괴 UX가 없어 해당 규칙은 별도 화면 행동으로 번역하지 않는다. | oh-my-principle commit 654485f96b7764c759662d2c3e9e386ebc221cf6 및 프로젝트 PERFORMANCE_TESTING·UI_BEHAVIOR 계약. |
| D-11 | 이슈와 현재 대화가 완결된 원천이며 해당 qa-log가 없으므로 Spec Gate는 원천 문서 부재로 건너뛴다. 준비 검사와 구현 후 독립 전체 계약 리뷰를 수행하고 human_approval은 pending을 유지한다. | gen-prd conversation-only 계약 및 사용자의 구현 위임. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 터미널에 `보드 보기 (docs/README.md)에 C안을 추가했습니다.`를 출력하고 실제 파일 경로를 hover하면 `docs/README.md`만 링크로 표시된다. 괄호와 조사에는 underline·click 범위가 생기지 않는다. | D-01, D-06 |
| B2 | 링크를 클릭하면 기존 파일 열기 경로로 정확한 파일을 열고, 기존 selection·modifier click 의미를 유지한다. | D-01, D-08 |
| B3 | 닫는 기호와 지원되는 조사 결합이 붙은 다른 경로도 문장 작성자가 공백·줄바꿈을 추가하지 않고 링크를 쓸 수 있다. 지원되지 않는 단어는 임의 절단하지 않는다. | D-03 |
| B4 | 조사나 닫는 기호를 실제 파일명에 포함한 원문 경로가 있으면 그 파일을 우선하고 해당 파일명의 전체 범위를 링크로 표시한다. | D-02, D-06 |
| B5 | 유한한 원문·기호·문법·위치 후보 중 여러 파일이 존재하면 가장 긴 실제 경로를 결정적으로 선택한다. 괄호·조사·위치가 함께 붙어도 조사만 제거한 기호 포함 파일명과 위치 문자열을 포함한 literal 파일명을 생략하지 않는다. 짧은 후보의 검사 응답이 먼저 도착해도 결과가 바뀌지 않는다. | D-02, D-04, D-07 |
| B6 | 모든 유효 후보가 존재하지 않으면 링크로 표시하거나 열지 않는다. 파일 검사 실패도 존재 확인으로 간주하지 않고 기존 진단 경로로 남긴다. | D-02, D-10 |
| B7 | URL과 OSC 8 링크, `path:line:column` 위치 지정, 파일명 내부 괄호·인용 기호 및 기존 absolute·home·cwd/root 상대 경로의 의미는 유지한다. | D-02, D-08 |
| B8 | line/column처럼 보이는 문자열 자체가 실제 파일명이면 원문 파일이 우선한다. 원문 파일이 없고 기존 위치 형식이 유효하면 기존 위치로 연다. | D-02, D-08 |
| B9 | 한국어 wide cell 앞뒤에 놓인 경로와 여러 terminal row에 걸친 경로에서도 정확한 시작·끝 cell과 실제 경로를 선택한다. | D-06 |
| B10 | 파일 검사와 새 suffix 해석은 hover 시 기존 provider 흐름에서만 실행되고 입력·렌더링·snapshot 갱신에 새 파일 검사가 발생하지 않는다. | D-04, D-07 |
| B11 | 한 joined spelling은 중복 제거 후 원문 포함 최대 6개 해석만 만든다. 토큰 하나의 원문 외 cwd/root lookup은 최대 160개이며 전체 lookup은 최대 192개다. 최대 16개 joined spelling을 실제로 만드는 wrapped fixture로 상한을 확인하며, 모든 글자를 하나씩 제거하는 파일 탐색은 발생하지 않는다. | D-03, D-04 |
| B12 | 한 hover 제공 호출은 최대 512개의 새로운 고유 경로만 host에 요청하고 각 요청 batch는 최대 64개다. 예산 초과 후보를 존재한다고 가정하지 않고 진단 수치를 남긴다. | D-05 |
| B13 | 동일 경로 반복 hover는 기존 TTL 동안 cache 결과를 쓰고, 512개를 넘는 retained cache는 기존 상한 안으로 정리된다. cache miss·unique logical lookup·native 파일 검사 횟수는 구분할 수 있다. | D-07 |
| B14 | remote context와 파일 검사 host가 없는 context에서는 새 로컬 파일 검사를 시도하지 않고 기존 URL·지원 링크 의미만 유지한다. | D-08 |
| B15 | 경로가 많은 한 줄의 cold·warm hover에서도 상한 내에서 링크가 결정되고, hover부터 표시까지의 시간과 unique lookup·batch·native 검사 건수를 같은 workload로 측정할 수 있다. | D-04, D-05, D-07 |
| B16 | 외부 경로와 executable 링크는 기존 등록 checkout 경계와 open/reveal 보안 정책을 거치며 이번 해석 때문에 권한 범위를 넓히지 않는다. | D-08, D-10 |
| B17 | 터미널 링크 안내 문서는 조사 결합·원문 우선·지원 경계와 기존 click 규칙을 실제 동작대로 설명한다. | D-01, D-03, D-08 |

## Technical structure

기존 parser가 원문 spelling과 정확한 terminal span을 유지하며 유한한 해석 후보를 제공하고, 기존 hover provider가 실제 존재·캐시·batch·budget을 통해 최종 링크를 결정한다.
실제 파일 확인과 파일 열기의 권한은 기존 native host와 현재 라우팅에 유지한다.
새 서비스·패키지·daemon filesystem API·원격 권한·persistent 저장소는 추가하지 않는다.

## Risks

- 문법처럼 보이는 실제 파일명을 잘못 줄일 수 있다. 원문과 단축 후보가 동시에 존재하는 충돌 fixture를 사용해 가장 긴 실제 경로 우선을 확인한다.
- wide Hangul의 문자열 index는 terminal cell 위치와 다르다. 실제 격리 desktop 후보의 링크 범위와 클릭 대상까지 확인하며 renderer 문자열 검사만으로 완료하지 않는다.
- 새 후보는 파일 확인 비용을 늘린다. 상한·dedup·cache를 유지하고 같은 경로 밀도의 cold/warm workload에서 기존 baseline과 후보의 latency·검사 건수를 비교한다.
- 512 새 lookup 예산을 넘는 병적인 줄은 일부 추론 링크가 표시되지 않을 수 있다. exact spelling을 우선하고 초과는 진단에 남긴다. 이 수치는 되돌릴 수 있는 위임 가정으로 사후 검토한다.
- 외부 앱 열기를 mock한 테스트만으로 실제 파일 열기가 검증되었다고 주장하지 않는다. 격리 candidate에서 실제 화면과 정확한 열린 파일을 관찰한다.
- 운영 앱·Herdr 서버·등록 checkout·사용자 파일을 바꾸지 않는다. 합성 경로·isolated HOME과 worktree 내 build를 사용하고 증거는 local-only로 둔다.
- 구현 전에 사용자 선행 작업은 없다. 사후 QA는 실제 한국어 업무 문장과 특이한 파일명·wrapping 확인이다.
