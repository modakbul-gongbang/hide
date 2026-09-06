---
topic: "Hide 디자인 시스템: modifier-held 단축키 힌트 컴포넌트, 커스텀 툴팁, 토큰 기반 셸 재설계"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "메인 윈도우 셸의 모든 표면을 다시 그리고 툴팁 렌더러를 교체하는 사용자 가시 변경이지만, 데이터, 자격 증명, 과금, 파괴적 동작, herdr-core 계약에는 손대지 않고 모든 라이브 효과가 격리된 dev 인스턴스 안에 머문다."
source_intake: "agents/interview/hide-design-system/qa-log.md"
created_at: "2026-09-06"
updated_at: "2026-09-06"
---

# PRD: Hide 디자인 시스템

## 1. Summary

⌘, ⌘⇧, ⌃, ⌘⌥ 같은 modifier를 누르고 있으면 정확히 그 조합에 걸린 chord가 해당 컨트롤 옆에 keycap으로 드러나고, 모든 툴팁이 "라벨 (chord)" 형식의 커스텀 다크 말풍선으로 바뀐다.
keycap, 툴팁, 힌트 칩은 command를 입력으로 받아 텍스트와 접근성 help를 한 곳에서 파생하는 하나의 컴포넌트가 되어, 재바인딩 뒤에도 stale chord가 남지 않는다.
같은 라운드에 메인 윈도우 셸 전체를 `HideTheme` 토큰과 분리된 컴포넌트로 다시 구성하고, 외형을 Orca 방향으로 재설계하며, DESIGN.md에 in-product 컴포넌트 섹션을 추가해 그 문서가 심사 기준이 되게 한다.
뷰 파일의 스타일 리터럴은 검사 스크립트와 invariant 규칙으로 기계 강제해 같은 drift가 다시 쌓이지 않게 한다.

Approval checklist:

- 범위: 토큰 추출과 시각 재설계를 한 번에 진행하고 기준선은 DESIGN.md 토큰 + Orca 외형이다 (section 3, D-09, D-10).
- 힌트 규칙: 정확 일치 modifier 조합만, 150ms 홀드 뒤 표시, 대상은 section 6의 매트릭스 R6이며 메뉴 전용 명령은 힌트가 없다 (D-12, D-28).
- 툴팁: 네이티브 `.help()`를 커스텀 말풍선으로 전면 교체하고 DESIGN.md Don't에 명시한다 (R4, R5, D-11, D-29).
- 구조 변경: 윈도우 수준 오버레이 레이어, 컴포넌트 파일 분리, 타이포그래피 스케일 토큰, 리터럴 invariant (section 5).
- 제외: 컨텍스트 메뉴, Pet 창과 메뉴바 대시보드, 새 컨트롤 추가 (non-goals N1-N4).
- 검증 모드: build/static, automated behavior, app runtime(판정형 스크린샷은 codex judge 쿼터 리셋 2026-09-07 이후) (section 9).
- 사람 승인 게이트 없음: 타이포 스케일과 치수는 4.3의 가정으로 확정하고 사용자는 완료 후 피드백한다 (D-16).
- 배포 모드: `agents/config.json`에 이미 설정된 local 모드를 그대로 따른다. push와 PR은 이 모드가 하지 않는 일이며 새 정책이 아니다 (4.3 저장소 설정 사실, A10).
- `review_profile: standard`와 그 근거 (frontmatter).

## 2. Problem, Goal, And Users

사용자는 Hide의 단일 운영자다.
운영자는 키보드로 셸을 움직이고 싶지만, 지금 어떤 컨트롤에 어떤 chord가 걸려 있는지는 탭의 ⌘n과 에이전트 행의 ⌃n 두 곳에서만 드러난다.
사이드바 버튼, 패널 토글, pane 헤더 버튼은 modifier를 눌러도 아무것도 보여주지 않고, 툴팁 7곳은 chord를 문자열로 하드코딩해 재바인딩 뒤 stale하게 남을 수 있는데, 이는 round3 PRD가 이미 레지스트리 참조를 요구했던 계약에서 벗어난 drift다.
툴팁 자체도 macOS 기본 노란 말풍선이라 다크 셸과 어울리지 않는다.

시각 값도 흩어져 있다.
main 기준으로 `hideFont(size:)`가 12종 154곳, 리터럴 padding 75곳, 리터럴 cornerRadius 17곳, 리터럴 opacity 26곳이 뷰 파일에 있고, `HideTheme`에는 타이포그래피 스케일이 없으며 keycap은 세 가지 다른 모양으로 그려진다.
앞선 PRD 두 개가 리터럴을 금지했지만 강제 장치가 없어 재발했다.

목표는 세 가지다.
운영자가 modifier를 누르는 순간 그 조합으로 할 수 있는 일이 화면에 보이고, 모든 툴팁이 같은 컴포넌트에서 나와 라벨과 chord를 함께 읽히며, 셸 전체가 문서화된 토큰으로 그려져 다음 변경이 값을 발명하지 않게 되는 것이다.

### 2.1 User Scenarios

- SC1. modifier 홀드로 단축키 힌트 드러내기: 운영자가 메인 윈도우에서 ⌘, ⌘⇧, ⌃, ⌘⌥ 중 하나를 150ms 넘게 누르고 있으면 그 조합과 정확히 일치하는 chord를 가진 컨트롤에만 keycap이 보인다.
  Actors: 운영자.
  Primary path: 탭에는 ⌘1-9, 에이전트 행에는 ⌃1-9가 인라인으로, 사이드바 새 에이전트에는 ⌘N, 새 프로젝트에는 ⌘⇧N(⌘⇧을 누를 때), 사이드바 접기와 펼치기에는 ⌘B, 뷰 스위처에는 ⌘E, 새 탭에는 ⌘T, 오른쪽 패널 토글에는 ⌘⇧B, 포커스 pane 헤더의 닫기와 확대 표시에는 유효 바인딩 chord가 떠 있는 칩으로 보이고 레이아웃은 밀리지 않는다. 손을 떼면 즉시 사라진다. 조합을 ⌘에서 ⌘⇧으로 바꾸면 노출 집합이 그 조합으로 즉시 바뀐다.
  Failure state: ⌘K 같은 일반 chord를 150ms 안에 치면 힌트가 번쩍이지 않는다. ⌘Tab으로 다른 앱에 가면 릴리즈 이벤트가 오지 않아도 힌트가 남지 않는다. Search, Settings, 새 에이전트 시트가 열리면 메인 윈도우 힌트는 지워진다. ⌃ 홀드 중에도 터미널로 가는 keyDown은 하나도 소비되지 않는다. 시스템 "동작 줄이기"가 켜져 있으면 페이드 없이 즉시 표시와 숨김이 일어난다.
  Recovery: Settings에서 pane chord를 재바인딩하면 다음 홀드부터 새 chord가 보이고, 탭 순서를 바꾸면 번호가 스트립을 따라간다.
  Reach: 격리된 dev 인스턴스에 탭 두 개 이상과 에이전트 행 두 개 이상이 있는 워크스페이스를 열고 실제 modifier를 누른 채 유지한다(합성 키 이벤트로 홀드를 만들어도 된다).

- SC2. 컨트롤 위에 머물러 툴팁 읽기: 운영자가 chorded 컨트롤이나 기존에 툴팁이 있던 컨트롤 위에 포인터를 잠시 두면 Orca 스타일의 짙은 둥근 말풍선이 붙어 나타난다.
  Actors: 운영자.
  Primary path: 새 에이전트 버튼 위에 머물면 "New agent (⌘N)", 포커스 pane의 닫기 위에서는 "Close this pane (⌘⇧W)", 사이드바 접기에서는 "Hide left sidebar (⌘B)"처럼 라벨과 chord가 함께 읽힌다. chord가 없는 컨트롤(리사이즈 핸들, 포트 링크, fork 표시, 기기 선택, 주간 사용량, Settings, 체크아웃 경로와 카드의 카운트, 파일 경로, 브라우저 pane의 새로고침과 CDP 복사)은 라벨만 보인다. VoiceOver는 같은 텍스트를 그 컨트롤의 help로 읽는다.
  Failure state: 포인터 이탈, mouseDown, 스크롤, keyDown, 윈도우 resign-key에 즉시 사라진다. 앵커 컨트롤이 사라지면 함께 사라진다. 윈도우 가장자리에서는 잘리지 않도록 반대쪽으로 붙는다. "동작 줄이기"면 페이드가 없다.
  Recovery: 재바인딩 직후 같은 컨트롤에 다시 머물면 새 chord가 보인다.
  Reach: 격리된 dev 인스턴스에서 포인터를 컨트롤 위로 옮기고 툴팁 지연 토큰보다 오래 유지한다.

- SC3. 재설계된 셸 사용하기: 운영자가 완성된 dev 인스턴스를 열어 사이드바, 탭 스트립, pane 헤더, 브라우저 pane 헤더, 오른쪽 패널, 상태 바, 빈 체크아웃 화면, Search 시트, Settings 시트, 파일 뷰어 오버레이를 본다.
  Actors: 운영자.
  Primary path: 모든 표면이 DESIGN.md in-product 섹션의 타이포 스케일, 4단 surface, hairline, radius와 spacing 토큰으로 그려지고 외형은 Orca 방향(밀도, keycap, 툴팁)이다. 기존 기능과 문구는 그대로 동작한다.
  Failure state: 빈 사이드바, 빈 체크아웃, pane 투영 불가, 크기 대기 중 pane, 오른쪽 패널 트리의 loading과 failed, 원격과 브라우저 phase 여섯 가지 전부(idle, loading, ready, stale, unavailable, failed)와 브라우저 pane 헤더의 "Connecting…"과 "Browser disconnected", 새 에이전트 시트의 비활성 시작 버튼, Settings의 비활성 추가 버튼, 편집기 충돌과 stale 배너가 모두 같은 토큰으로 그려지고 동작과 문구는 그대로다. Pet 창과 메뉴바 대시보드는 바뀌지 않는다.
  Recovery: 취향 불일치는 실패가 아니라 완료 후 피드백이며 후속 라운드가 흡수한다.
  Reach: 격리된 dev 인스턴스에서 빈 워크스페이스, 에이전트가 있는 워크스페이스, 브라우저 pane, 시트 세 개, 오른쪽 패널의 두 섹션을 차례로 연다. 비정상 상태는 기존 fixture와 소켓 격리로 만든다.

## 3. Scope And Non-Goals

포함:

- 힌트 컴포넌트: command를 입력으로 받아 keycap 텍스트, 툴팁 텍스트, accessibilityHelp를 한 곳에서 파생하는 하나의 말풍선 컴포넌트(툴팁 모드와 힌트 칩 모드)와 인라인 keycap.
- modifier 홀드 상태의 일반화: 현재 ⌘와 ⌃ 두 개의 불리언에서 modifier 집합 하나로.
- 힌트 대상 매트릭스(R6)에 있는 모든 컨트롤과, 메인 셸의 `.help()` 28곳 전부의 커스텀 툴팁 이관.
- 메인 윈도우 셸 전체의 토큰화와 재설계: 사이드바(브랜드 헤더, 검색과 명령 바, 뷰 스위처, 워크스페이스와 체크아웃 행, 체크아웃 요약 카드, 에이전트 행, 유틸리티 바), 탭 스트립, 터미널 pane 헤더, 브라우저 pane 헤더와 연결 상태, 오른쪽 패널(헤더, 파일 트리, Changes), 상태 바와 그 원격·브라우저 phase 표시(idle, loading, ready, stale, unavailable, failed 여섯 가지 전부), 빈 체크아웃 화면, pane 투영 불가 상태, Search 시트, 새 에이전트 시트, Settings 시트, 파일 뷰어 오버레이, 편집기 배너. 이 목록은 D-17의 "메인 윈도우 셸 전부"를 main 기준으로 열거한 것이며, 브라우저 pane과 체크아웃 요약 카드는 인터뷰 뒤 main에 합쳐진 메인 셸 표면이라 같은 규칙으로 포함된다.
- `HideTheme` 확장: 타이포그래피 스케일, 툴팁 지연, keycap과 말풍선 치수, Orca 외형에 맞춘 토큰 값 갱신.
- 컴포넌트 파일 분리: `HideUI.swift`에서 토큰, keycap, 말풍선, 아이콘 버튼, 배지를 각자 파일로.
- DESIGN.md: in-product 컴포넌트 섹션 추가, 네이티브 툴팁 Don't 항목, Known Gaps와 hover 정책 문구 갱신, lint 0 오류 0 경고.
- 리터럴 검사 스크립트와 invariant 규칙 등록.
- 접근성: 모든 chorded 컨트롤의 help 속성이 툴팁 텍스트와 같고, "동작 줄이기" 존중.

제외(non-goals):

- N1. 컨텍스트 메뉴 재설계(아이콘 항목, 빨간 파괴적 항목). 결과: 우클릭 메뉴는 지금의 네이티브 `NSMenu` 외형으로 남는다. 근거: 툴팁·힌트·토큰만으로 판정형 검증이 무겁고 현재 메뉴 목록 조사가 선행돼야 한다(D-14). 재방문: 이 PRD 출하 후 사용자가 메뉴 재설계를 요청할 때.
- N2. Pet 창과 메뉴바 대시보드. 결과: `PetTheme`과 `PetView.swift:163`의 네이티브 툴팁이 그대로 남는다. 근거: 성격이 다른 별도 테마이며 사용자가 Q9에서 포함 선택지를 기각했다(D-17, D-34). 재방문: 완료 후 피드백에서 요청될 때.
- N3. 메뉴 전용 명령의 힌트(⌘P 파일 열기, ⌘F 찾기, ⌘D/⌘⇧D 분할, ⌘= ⌘- ⌘0 글자 크기). 결과: 이 명령은 앱 메뉴에서만 chord를 보인다. 근거: 보이는 컨트롤이 없다(D-28). 재방문: 해당 컨트롤이 생길 때.
- N4. 새 컨트롤 추가(예: pane 헤더 분할 버튼). 근거: 힌트를 위해 컨트롤을 늘리는 것은 범위 확장이다(D-28).
- N5. herdr-core의 런타임 동작, C ABI, snapshot 필드 변경. 힌트와 툴팁 상태는 셸에 산다. 이 run이 herdr-core에서 바꾸는 것은 `herdr-core/src/runtime.rs`의 `#[cfg(test)] mod tests` 안 6줄뿐이다: 셸 파일 배치를 비추는 테스트가 `tabStripHeight`와 `trafficLightInset`를 승인된 컴포넌트 분리(D-21)에 따라 `HideUI.swift` 대신 `HideTheme.swift`에서 읽도록 읽기 경로를 바꾼 것으로, 테스트 전용 변경이며 런타임 코드, C ABI, snapshot에는 손대지 않는다. 이 파일이 run-owned 변경 목록에 오르는 이유는 그 6줄이 전부이고, 이는 이 non-goal 위반이 아니다.
- N6. 라이트 모드, 새 애니메이션 언어, 새 외부 의존성.

## 4. Pre-Work And Required Decisions

### 4.1 Pre-Work Before Implementation

None required.
자격 증명, 계정, 외부 자산이 필요하지 않고 모든 준비(스크립트, 토큰, 문서, fixture)는 에이전트가 만든다.

### 4.2 Human Decisions Before PRD Approval

None required.
모든 제품 결정은 qa-log의 사용자 결정(D-09..D-22, D-28..D-31)에 근거하고, 남은 선택은 4.3의 되돌리기 쉬운 에이전트 가정으로 기록했다.
사용자는 `/please`로 승인 왕복을 위임했고 완료 후 전체를 보고 피드백한다(D-16).

### 4.3 Decision Traceability For Fidelity Review

사용자 결정(qa-log Decision Register):

- D-09 토큰 추출과 시각 재설계를 함께 진행, 픽셀 동일 보존은 목표 아님 (범위 section 3, R1, 위험 section 10). 기각: 픽셀 동일 추출 + 경계 지은 신규 레이어, 시각 변화 0.
- D-10 시스템은 DESIGN.md/HideTheme로 문서화하고 in-product 섹션을 추가하되 외형 목표는 Orca이며 충돌 시 토큰 값을 Orca 쪽으로 갱신 (R2, R3, AC12, AC13). 기각: Orca를 별도 시스템으로 복제.
- D-11 커스텀 다크 툴팁, 라벨 (chord) 텍스트 병기, 메인 셸의 `.help()` 전부 이관, 네이티브 툴팁은 DESIGN.md Don't (R4, R5, AC5, AC6, AC13). 기각: 네이티브 유지.
- D-12 정확 일치 modifier 조합만 드러냄, 150ms 지연, 릴리즈 즉시 숨김, 비활성화 해제 유지 (R6, R8, AC1, AC2). 기각: 포함하는 모든 chord, 탭·에이전트 번호만.
- D-13, D-18 아이콘 전용 컨트롤은 떠 있는 keycap 칩, 라벨 있는 컨트롤은 인라인 keycap, 같은 keycap 스타일과 같은 말풍선 렌더러 (R7, AC3). 기각: 아이콘 교체, 모서리 배지.
- D-14 컨텍스트 메뉴 제외 (N1). 기각: 커스텀 메뉴 포함.
- D-15 뷰 리터럴 검사 스크립트를 invariant로 등록해 gate와 CI에서 실행, 예외는 토큰 추가로만 (R11, AC10, AC11, T9). 기각: AC로만 두기.
- D-16 사용자 승인 게이트 없음, 에이전트가 스케일과 치수를 정하고 사용자는 완료 후 피드백 (9.3, A2-A5).
- D-17, D-34 메인 윈도우 셸 전부 포함, Pet 창과 메뉴바 대시보드 제외 (section 3, N2, AC14). 기각: Pet 포함, 세 표면만.
- D-19 툴팁 지연은 macOS 기본보다 짧은 토큰, 힌트 칩은 150ms (R4, A3).
- D-20 "동작 줄이기"면 페이드 생략 (R9, AC4, AC7).
- D-21 컴포넌트를 `HideUI.swift`에서 분리 (section 5, R10, T2).
- D-22 DESIGN.md lint를 검증에 포함 (AC13, V1).
- D-28 힌트 대상 매트릭스와 메뉴 전용 명령 제외, pane/탭 명령은 포커스 대상 하나에만 (R6, AC1, N3, N4).
- D-29 메인 셸 `.help()` 전부 이관, Pet 1곳 제외, chord 없는 곳은 라벨만 (R4, AC5, AC6).
- D-30 오버레이 생명주기: 앵커 귀속, 스냅샷 재계산, 시트 열림 시 해제, 툴팁 닫힘 조건 (R8, AC2, AC7).
- D-31 비정상 상태 전부 같은 토큰으로 재설계, 동작과 문구 불변 (R1, AC14, SC3).

저장소 사실(qa-log D-01..D-08, D-23..D-27, D-32, D-33; 구현 기준 main에서 재측정):

- 측정 기준: qa-log는 인터뷰 브랜치(a90f61b)에서 쟀고 구현 기준은 main(caaa665)이다. D-29의 사용자 결정은 "메인 셸의 `.help()` 전부를 이관하고 Pet 1곳만 제외"라는 규칙이며 20이라는 숫자는 그날 그 브랜치의 측정값이다. main에는 그 뒤 브라우저 pane(`BrowserPaneView`, 2곳)과 체크아웃 요약 카드(`CheckoutSummaryCard`, 7곳)가 합쳐져 같은 규칙으로 세면 29곳 중 28곳이 이관 대상이다. 이는 저장소 사실에 따른 갱신이지 결정 변경이 아니며, 최종 이관 목록은 main의 메인 셸 `.help()` 전수와 같다. 같은 이유로 `hideFont(size:)` 154곳 12종, 리터럴 padding 75곳, cornerRadius 17곳, opacity 26곳도 main 측정값이다.
- 저장소 설정 사실(qa-log D 번호 없음): 배포 경계는 `agents/config.json`의 기존 설정이다: `delivery.mode` local, `baseBranch` main, `worktree.enabled` true. local 모드는 receipt 뒤 로컬 커밋 하나를 만들고 push, PR, CI watch, merge를 하지 않는다(`/please` Stage 3 규칙). 이 PRD는 그 설정을 바꾸지 않는다.
- 저장소 사실(수정 3, 사용자 승인 "승인" 및 "origin/main의 수정본 채택"): (1) `herdr-core/src/runtime.rs`의 `#[cfg(test)] mod tests`는 셸 상수 `tabStripHeight`, `trafficLightInset`를 소스 파일에서 읽어 검증하므로 D-21의 컴포넌트 분리 뒤에는 `HideTheme.swift`를 읽어야 한다. 이 6줄은 테스트 전용 변경이며 런타임 코드가 아니다 (N5, section 5, section 11). (2) `scripts/check-capability-readers-off-lock.sh`는 caaa665 자체 트리에서 exit 1로 실패한다(테스트 픽스처의 `Command::new("git")` 3곳을 런타임 fork로 오인). origin/main 2cb94d7이 테스트 모듈을 예외 처리해 고쳤으므로 이 run은 그 본문을 그대로 채택한다 (AC16, section 10).
- D-01 modifier 홀드 힌트는 `ShellModel.setShortcutModifiersHeld`(⌘, ⌃ 두 불리언, 150ms Task)와 `HerdrApp.swift`의 로컬 키 모니터에 있고 `applicationDidResignActive`가 해제한다 (R8, T3).
- D-02 keycap 세 렌더링: `SidebarBadge`(8pt, 16h), `HideSettingsKeycaps`(10pt mono, 18h), 인라인 `Text("⌘K")` (R7, T2).
- D-03 하드코딩 chord 툴팁 7곳과 `ShellMenuCommand.displayShortcut`, `PaneShortcut.displayString` (R4, R10, AC6).
- D-06 레지스트리 두 개: `ShellMenuCommand` 10개 고정 chord, `PaneCommand` 7개 재바인딩 가능(유효 바인딩은 `ShellModel.paneShortcuts`), 직접 선택 ⌘1-9/⌃1-9. ⌃는 터미널 트래픽이라 모니터는 관찰만 한다 (R6, R8, AC1, AC9).
- D-07, D-26 판정형 스크린샷은 codex judge 백엔드가 필요하고 쿼터는 2026-09-07 리셋. dev 번들 인스턴스가 정확히 하나여야 한다 (section 10, 11).
- D-23 컴포넌트는 command를 받고 문자열을 받지 않는다 (R10, AC6).
- D-24 힌트는 유일한 경로가 아니며 help 속성이 툴팁과 같다 (R12, AC8).
- D-25 키 이벤트 통과 규칙 유지 (R8, AC9).
- D-27 구현 순서는 토큰 추출 먼저, 그 위에 재설계 (T4, T5, section 10).
- D-32 힌트 상태기계와 툴팁 컨트롤러의 기계 테스트 (AC2, AC7, V2).
- D-33 리터럴 검사 범위는 메인 셸 파일이며 `Pet*.swift`와 토큰 정의 파일은 제외 (R11, AC10).

에이전트 가정(되돌리기 쉬움, 사용자 veto 대상):

- A1. 구현 기준은 로컬 `main`(caaa665)이다. 이 세션에서 origin fetch가 실패해 원격 최신은 확인하지 못했다. 재방문: 원격에 더 새 main이 있으면 rebase.
- A2. 타이포그래피 스케일은 7단이다: micro 9(keycap, 배지), caption 10, body 11, subhead 12, title 13, headline 17, display 30. 기존 8, 15, 16, 18, 19pt 사용처는 가장 가까운 단으로 옮긴다. 근거: 현재 사용 빈도가 10, 9, 11, 12에 몰려 있고 17과 30은 빈 상태 화면의 제목이다.
- A3. 툴팁 hover 지연 토큰은 400ms, 힌트 칩은 150ms 유지, 페이드는 120ms. 근거: Orca 스크린샷의 즉답성과 일반 chord의 번쩍임 방지 사이의 값.
- A4. keycap 치수는 높이 18, micro 9pt monospaced medium, `radiusSmall`, `elevated` 배경에 hairline 테두리로 통일한다(현재 Settings keycap의 외형을 기준). 말풍선은 `elevated`보다 한 단 밝은 새 surface 토큰, `radiusMedium`, hairline, 그림자 없음, subhead 12pt.
- A5. 떠 있는 칩은 기본적으로 컨트롤 위쪽 중앙에 붙고 위에 자리가 없으면 아래쪽에 붙는다. 툴팁도 같은 규칙이며 좌우는 윈도우 안으로 밀어 넣는다.
- A6. DESIGN.md frontmatter의 `属于:` 키(3행)는 lint가 거부하는 중첩 매핑이라 `essence:`로 바꾼다. 내용은 그대로다. 근거: AC13의 "0 경고"를 기계 검사 가능하게 한다.
- A7. 오버레이는 SwiftUI 앵커 프리퍼런스로 컨트롤 프레임을 모아 윈도우 콘텐츠 최상위 ZStack에서 그린다. NSPanel이나 별도 윈도우는 쓰지 않는다. 근거: 포커스와 키 윈도우 상태를 건드리지 않는다.
- A8. 힌트 칩의 타깃은 pane 명령이면 `focusedPaneID`, 탭 닫기면 활성 탭이며 스냅샷이 바뀌면 다시 계산한다(D-30).
- A9. 리터럴 검사 스크립트는 `scripts/check-hide-theme-literals.sh`이고 `.opacity(0)`과 `.opacity(1)`은 허용한다(표시와 숨김). 검사 대상 패턴은 R11에 있다.
- A10. 사용자의 "worktree 최신기준으로 파서"는 harness 워크트리 관례(`prd/<slug>` 브랜치, 기준은 A1의 로컬 main)로 읽는다. 배포 모드 자체는 위 저장소 설정 사실이며 이 가정은 브랜치 이름과 기준점만 정한다. 재방문: 사용자가 다른 브랜치나 원격 기준을 원할 때.

## 5. Major Technical Structure Changes

- 윈도우 수준 오버레이 레이어: 툴팁과 힌트 칩을 그리는 새 레이어가 메인 윈도우 콘텐츠 최상위에 생기고, 컨트롤은 앵커 프리퍼런스로 자기 프레임과 command를 올린다. 지금은 hover 추적 6곳과 네이티브 `.help()`뿐이고 오버레이 인프라가 없다.
- 힌트 상태의 일반화: `ShellModel`의 ⌘/⌃ 불리언 두 개와 Task 두 개가 "현재 홀드 중인 modifier 집합과 그 집합이 150ms를 넘겼는지"를 담는 상태 하나로 바뀌고, 노출 집합은 그 집합과 레지스트리, 포커스로부터 순수 함수로 계산된다.
- 컴포넌트 라이브러리 분리: `HideUI.swift`(2,239줄)에서 토큰(`HideTheme`), keycap, 말풍선, 아이콘 버튼, 배지가 각자 파일로 나가고 뷰 파일은 조립만 한다. `SidebarBadge`, `HideSettingsKeycaps`, 인라인 keycap 텍스트는 하나의 keycap 컴포넌트로 합쳐지고 나머지는 삭제된다.
- `HideTheme` 확장: 타이포그래피 스케일, 툴팁 지연, 말풍선 surface와 치수, Orca 외형에 맞춘 값 갱신. 셸의 모든 시각 값이 이 파일에서만 정의된다.
- 기계 강제: `scripts/check-hide-theme-literals.sh`, `scripts/check-hide-components.sh`와 `agents/rules/invariants/INV-hide-theme-literals.md`가 생기고 `rules add`로 ledger에 등록된다. CI 워크플로가 이 검사를 호출한다.
- DESIGN.md: in-product 컴포넌트 섹션과 Don't 항목이 추가되고 Known Gaps의 in-product·hover 문장이 갱신된다.
- herdr-core의 런타임 동작, C ABI, snapshot 포맷, `PetTheme`은 바뀌지 않는다. herdr-core 안에서 허용되는 유일한 변경은 `herdr-core/src/runtime.rs`의 `#[cfg(test)] mod tests` 읽기 경로 6줄(테스트 전용, 런타임 코드 아님)이다. `scripts/check-capability-readers-off-lock.sh`는 기준 커밋 caaa665 자체 트리에서 이미 실패하는 false positive(테스트 픽스처의 git 호출을 런타임 fork로 오인)를 가지므로, 이 run은 origin/main 2cb94d7이 고친 스크립트 본문을 바이트 그대로 채택한다. 이는 기준 커밋 결함의 수용이지 새 검증 정책이 아니다.

## 6. Requirements

- R1. 메인 윈도우 셸의 모든 표면(section 3 포함 목록)과 그 비정상 상태(D-31)는 `HideTheme` 토큰과 분리된 컴포넌트로만 그려지고 외형은 Orca 방향으로 재설계된다. 동작, 문구, 접근성 식별자, 기존 기능은 바뀌지 않는다.
- R2. DESIGN.md에 in-product 컴포넌트 섹션이 추가된다: 타이포그래피 스케일, surface 4단과 말풍선 surface, keycap, 툴팁과 힌트 칩, 아이콘 버튼, 배지, 시트와 오버레이. 각 항목은 토큰 참조로 값을 말하고 `HideTheme`의 값과 같다. Known Gaps의 "in-product chrome 미문서"와 "hover 미문서" 문장은 삭제되거나 새 섹션을 가리키도록 고쳐진다.
- R3. `HideTheme` 토큰 값이 Orca 외형과 충돌하면 토큰 값이 갱신되고 DESIGN.md가 함께 바뀐다. 뷰에서 값을 덮어쓰지 않는다.
- R4. 툴팁은 커스텀 말풍선 컴포넌트가 그린다. 메인 셸의 `.help()` 28곳은 전부 이 컴포넌트로 옮겨지고, chord가 있는 컨트롤은 "라벨 (chord)"를, 없는 컨트롤은 라벨만 보인다. hover 지연, 닫힘 조건(포인터 이탈, mouseDown, 스크롤, keyDown, 윈도우 resign-key), 가장자리 배치는 컴포넌트가 책임진다.
- R5. DESIGN.md Don't에 "네이티브 `.help()` 툴팁을 쓰지 않는다"가 추가되고, 메인 셸 뷰 파일에 `.help(` 호출이 남지 않는다. `PetView.swift`는 제외한다.
- R6. modifier 홀드 힌트 매트릭스. 홀드 중인 조합과 정확히 일치하는 chord만 드러난다.

  | 컨트롤 | command | chord(기본) | 힌트 모드 |
  | --- | --- | --- | --- |
  | 사이드바 검색 버튼 | `ShellMenuCommand.search` | ⌘K | 인라인 keycap(상시 표시, 홀드 시 강조) |
  | 사이드바 새 에이전트 | `.newAgent` | ⌘N | 떠 있는 칩 |
  | 사이드바 새 프로젝트 | `.newWorkspace` | ⌘⇧N | 떠 있는 칩 |
  | 사이드바 뷰 스위처 | `.toggleSidebarView` | ⌘E | 떠 있는 칩 |
  | 사이드바 접기, 탭 스트립의 펼치기 | `.toggleLeftSidebar` | ⌘B | 떠 있는 칩 |
  | 탭 1-9 | 직접 선택 번호 | ⌘1-9 | 인라인 keycap |
  | 활성 탭의 닫기 | `.closeTab` | ⌘W | 떠 있는 칩(활성 탭만) |
  | 새 탭 | `.newTab` | ⌘T | 떠 있는 칩 |
  | 오른쪽 패널 토글, 패널 헤더의 접기 | `.toggleRightPanel` | ⌘⇧B | 떠 있는 칩 |
  | 포커스 pane 헤더의 닫기 | `PaneCommand.closePane` 유효 바인딩 | ⌘⇧W | 떠 있는 칩(포커스 pane만) |
  | 포커스 pane 헤더의 확대 표시(확대 중일 때만 존재) | `PaneCommand.toggleZoom` 유효 바인딩 | ⌘⌥↩ | 떠 있는 칩(포커스 pane만) |
  | 에이전트 행 1-9 | 직접 선택 번호 | ⌃1-9 | 인라인 keycap |

  fork, 포트 링크, 브라우저 pane의 새로고침과 CDP 복사, 체크아웃 카드의 버튼은 chord가 없어 툴팁만 있다. 메뉴 전용 명령(N3)은 힌트가 없다.
- R7. 인라인 keycap과 떠 있는 칩은 같은 keycap 스타일(A4)을 쓰고, 칩은 툴팁과 같은 말풍선 렌더러의 두 번째 모드다. 칩은 레이아웃을 밀지 않는다.
- R8. 힌트 생명주기: 홀드 150ms 뒤 표시, 릴리즈 즉시 숨김, 앱 비활성화 시 해제, 조합 변경 시 노출 집합 교체, 시트(Search, Settings, 새 에이전트) 열림 시 해제와 닫힘 시 modifier 재읽기, 스냅샷 변경 시 재계산, 앵커 소멸 시 함께 소멸. 로컬 키 모니터는 flagsChanged를 관찰만 하고 어떤 keyDown도 새로 소비하지 않는다.
- R9. `accessibilityReduceMotion`이 켜져 있으면 힌트와 툴팁의 페이드가 0이 된다.
- R10. 힌트 컴포넌트의 입력은 command다: `ShellMenuCommand`, `PaneCommand`(유효 바인딩으로 해석), 직접 선택 번호, 또는 chord 없는 라벨. 어떤 호출 지점도 "(⌘X)" 텍스트를 조합하지 않으며 하드코딩 chord 문자열 7곳은 삭제된다. 재바인딩 뒤 keycap, 툴팁, help가 모두 새 chord를 보인다.
- R11. `scripts/check-hide-theme-literals.sh`는 `macos/Sources/HerdrMacOS/*.swift`에서 `Pet*.swift`와 토큰 정의 파일을 제외한 파일에 다음이 있으면 실패한다: 숫자 인자의 `hideFont(size:`, 숫자 인자의 `.padding(`, 숫자의 `cornerRadius:`, 0과 1이 아닌 숫자의 `.opacity(`, `Color(red:`, `Color.white.opacity(`, `Color.black.opacity(`, `.help(`, modifier 기호 리터럴(⌘⌃⌥⇧). 스크립트는 양성 fixture(리터럴 하나를 넣은 임시 파일)에서 실패하고 완성 트리에서 0건으로 통과한다. `agents/rules/invariants/INV-hide-theme-literals.md`가 트리거 경로와 이 명령으로 등록되고 CI 워크플로가 호출한다.
- R12. 모든 chorded 컨트롤과 툴팁이 있는 컨트롤의 accessibilityHelp는 툴팁 텍스트와 같다. 앱 메뉴는 모든 chord를 계속 나열한다.
- R13. 자동 회귀 테스트가 보호하는 위험: (a) 노출 집합 계산이 조합·레지스트리·포커스와 어긋남, (b) 홀드 상태기계의 지연·릴리즈·비활성화·전환·시트·스냅샷 재계산·앵커 소멸 규칙 회귀, (c) 툴팁 컨트롤러의 지연·닫힘·가장자리·reduced-motion 회귀, (d) 재바인딩 뒤 stale chord, (e) 뷰 리터럴 재유입, (f) 키 이벤트 통과 회귀, (g) DESIGN.md 토큰 값과 `HideTheme` 값의 불일치, (h) 문구와 접근성 식별자의 의도치 않은 변경, (i) 컴포넌트가 뷰 파일로 다시 복제되는 구조 회귀.
- R14. AGENTS.md Design Reference가 컴포넌트 파일 위치, 툴팁 규칙, 리터럴 검사 스크립트를 한 문단으로 기술한다.
- R15. `scripts/check-hide-components.sh`는 컴포넌트 분리를 정적으로 검사한다: (a) 토큰 파일, keycap, 말풍선(툴팁·칩), 아이콘 버튼, 배지 컴포넌트가 각자 이름 붙은 파일에 있고 그 파일이 해당 타입을 정의한다, (b) `HideUI.swift`는 그 타입을 더 이상 정의하지 않고 `HideSettingsKeycaps` 심볼과 `SidebarBadge`의 keycap 용도가 트리에 없다, (c) keycap 글리프 그리기와 말풍선 그리기가 컴포넌트 파일 밖에 없다, (d) 4.3의 `.help()` 28곳이 있던 메인 셸 뷰 파일이 모두 툴팁 modifier를 참조하고 그 참조 수의 합이 28 이상이다. 스크립트는 위반 0건으로 통과하고, 컴포넌트 타입 하나를 뷰 파일에 복제한 양성 fixture에서 실패한다.

## 7. Acceptance Criteria

| ID | Criterion | Judgment | Evidence Declaration |
| --- | --- | --- | --- |
| AC1 | 노출 집합 계산은 ⌘ 단독, ⌘⇧, ⌃, ⌘⌥ 각 조합에 대해 R6 매트릭스와 정확히 같은 컨트롤 집합을 내고, 다른 조합(예: ⌥ 단독, ⇧ 단독)에는 빈 집합을 내며, pane 명령과 탭 닫기는 포커스 pane과 활성 탭 하나만 대상으로 하고, 재바인딩된 pane chord는 유효 바인딩으로 표시된다 | machine | - |
| AC2 | 홀드 상태기계는 150ms 전 릴리즈에 표시하지 않고, 150ms 뒤 표시하며, 릴리즈에 즉시 숨기고, resign-active에 해제하며, ⌘에서 ⌘⇧으로 바뀌면 집합을 교체하고, 시트 열림에 해제하며 닫힘에 modifier를 다시 읽어 홀드 중이면 새 집합을 표시하고, 홀드 중 스냅샷이 바뀌면(포커스 pane 변경, 탭 재정렬, 활성 탭 변경, 확대 해제) 칩의 대상이 새 대상으로 교체되며, 앵커 컨트롤이 사라지면(pane 닫힘, 탭 닫힘) 그 칩이 노출 집합에서 빠진다 | machine | - |
| AC3 | 격리된 dev 인스턴스에서 ⌘, ⌘⇧, ⌃, ⌘⌥을 각각 누르고 있으면 R6의 해당 컨트롤에만 keycap이 보이고 레이아웃이 밀리지 않으며, 인라인 keycap과 떠 있는 칩이 같은 keycap 스타일이고, 손을 떼면 사라진다 | judged | scripted run: hold each of the four combinations past the delay, capture the whole window for each, capture once more after release, and record the hint diagnostics for the run |
| AC4 | "동작 줄이기"가 켜진 상태에서 힌트와 툴팁은 페이드 없이 즉시 나타나고 사라진다 | machine | - |
| AC5 | 메인 셸의 컨트롤 위에 툴팁 지연보다 오래 머물면 커스텀 말풍선이 "라벨 (chord)" 또는 라벨만으로 나타나고, 포인터 이탈과 클릭에 사라지며, 윈도우 오른쪽 가장자리의 컨트롤에서도 잘리지 않는다 | judged | scripted run: hover the new agent button, the focused pane close button, a chord-less browser header button, and a control at the window edge; capture each tooltip and the state after pointer exit |
| AC6 | 모든 chorded 컨트롤의 툴팁 텍스트와 keycap 텍스트는 레지스트리 `displayString`과 같고, pane chord를 재바인딩하면 셋 다 새 chord를 보이며, 뷰 파일에 하드코딩 chord 문자열이 없다 | machine | - |
| AC7 | 툴팁 컨트롤러는 지연 토큰 뒤 표시, 포인터 이탈·mouseDown·스크롤·keyDown·resign-key에 즉시 닫힘, 가장자리에서 반대편 배치, reduced-motion에서 애니메이션 0을 결정론적으로 만족한다 | machine | - |
| AC8 | R6의 모든 컨트롤과 툴팁이 있는 모든 컨트롤의 accessibilityHelp가 툴팁 텍스트와 같다 | machine | - |
| AC9 | ⌃ 홀드로 힌트가 표시된 동안 로컬 키 모니터는 keyDown을 통과시키고, 기존 switcher와 close chord 동작은 그대로다 | machine | - |
| AC10 | `scripts/check-hide-theme-literals.sh`가 완성 트리에서 0건으로 통과하고, 리터럴 하나를 넣은 양성 fixture에서 실패하며, `Pet*.swift`와 토큰 정의 파일의 리터럴은 세지 않는다 | machine | - |
| AC11 | `agents/rules/invariants/INV-hide-theme-literals.md`가 `rules add`로 등록되어 `agents/rules/INDEX.md`에 행을 갖고, CI 워크플로가 검사 스크립트를 호출한다 | machine | - |
| AC12 | 격리된 dev 인스턴스의 대표 상태 스크린샷 세트(사이드바 정상과 빈 상태, 체크아웃 카드, 탭 스트립, 터미널과 브라우저 pane 헤더, 브라우저 연결 중과 끊김, 오른쪽 패널 두 섹션과 트리 loading/failed, 상태 바의 원격·브라우저 phase 여섯 가지(idle, loading, ready, stale, unavailable, failed) 각각, 빈 체크아웃, pane 투영 불가, 크기 대기 중 pane, Search·새 에이전트·Settings 시트와 그 비활성 버튼, 파일 뷰어 오버레이와 편집기 충돌·stale 배너)가 DESIGN.md in-product 섹션의 토큰과 일치하고 Orca 방향의 밀도와 keycap을 보이며, AC19의 인벤토리 비교가 뒷받침하듯 문구가 바뀌지 않았다 | judged | the screenshot set read against the DESIGN.md in-product section and the pre-change set, with the state each frame is in named |
| AC13 | DESIGN.md에 in-product 컴포넌트 섹션, 네이티브 툴팁 Don't, 갱신된 Known Gaps가 있고 `npx @google/design.md lint DESIGN.md`가 0 오류 0 경고다 | machine | - |
| AC14 | Pet 창과 메뉴바 대시보드의 전후 스크린샷이 같고 `PetTheme`과 `PetView.swift`의 툴팁이 바뀌지 않았다 | judged | before and after captures of the pet window and the menu bar dashboard, and the diff for the Pet files |
| AC15 | 토큰 추출 단계(T4) 끝의 스크린샷 세트가 시작 시점 세트와 픽셀 동일하고, 그 뒤 재설계(T5)에서만 픽셀이 바뀐다 | judged | two screenshot sets diffed against the baseline set, with the diff summary per frame |
| AC16 | 셸 빌드와 테스트, core 테스트, 기존 계약 검사, 리터럴 검사, DESIGN.md lint가 모두 통과한다 | machine | - |
| AC17 | AGENTS.md Design Reference가 컴포넌트 파일 위치, 툴팁 규칙, 리터럴 검사를 기술한다 | judged | the AGENTS.md diff of the run read against R14 |
| AC18 | 셸 테스트가 DESIGN.md frontmatter의 color, spacing, radius 토큰 값과 typography 표의 크기를 파싱해 `HideTheme`의 같은 이름 토큰 값과 비교하고 전부 일치하며, in-product 섹션이 이름으로 부르는 토큰이 `HideTheme`에 모두 존재한다 | machine | - |
| AC20 | `scripts/check-hide-components.sh`가 완성 트리에서 R15의 (a)-(d)를 위반 0건으로 통과하고, 컴포넌트 타입을 뷰 파일에 복제한 양성 fixture에서 실패한다 | machine | - |
| AC19 | 기준선(T1 시점)과 완성 트리에서 메인 셸 소스의 accessibilityIdentifier 문자열, accessibilityLabel 문자열, 사용자 가시 문구(`Text` 리터럴과 `.help()`에서 이관된 라벨)를 추출한 인벤토리를 비교하면, `.help()` 문자열이 같은 라벨의 툴팁 인자로 옮겨진 것 외에 추가, 삭제, 변경이 없고, 기존 셸 상호작용 테스트(`ShellModel`, `ShellMenuCommand`, `PaneShortcutSettings`, 탭과 pane 동작)가 모두 통과한다 | machine | - |

## 8. PRD-Level Tasks

- T1. 기준선을 만든다: 격리된 dev 인스턴스(HERDR_SOCKET_PATH, 인스턴스 하나)에서 AC12의 상태 목록과 Pet 창, 메뉴바 대시보드를 찍고, 메인 셸 소스의 문구와 접근성 식별자 인벤토리를 추출해 `agents/runs/hide-design-system/` 아래에 둔다. Covers AC14, AC15, AC19. Depends on: none.
- T2. 토큰과 컴포넌트를 분리한다: `HideTheme`을 자기 파일로 옮기고 타이포그래피 스케일, 툴팁 지연, 말풍선 토큰을 추가하며, keycap 하나(`SidebarBadge`·`HideSettingsKeycaps`·인라인 keycap 통합), 말풍선(툴팁·칩 모드), 아이콘 버튼, 배지를 각자 파일로 만든다. 컴포넌트 분리 검사 스크립트 `scripts/check-hide-components.sh`를 양성 fixture와 함께 만든다(그 (d) 항목은 T5의 이관 뒤에야 참이므로 AC20은 T5가 닫는다). Covers R7, R10, R15, D-21. Depends on: none.
- T3. 힌트 상태와 노출 집합을 일반화한다: `ShellModel`의 modifier 상태를 집합 하나로 바꾸고 노출 집합 순수 함수와 상태기계 테스트를 쓴다. 로컬 키 모니터의 통과 규칙 테스트를 추가한다. Covers R6, R8, R13, AC1, AC2, AC9. Depends on: T2.
- T4. 리터럴을 토큰으로 흡수한다: 메인 셸 뷰 파일의 hideFont, padding, cornerRadius, opacity, 색 리터럴을 `HideTheme`으로 옮기고 외형은 그대로 둔다. 끝에서 스크린샷 세트를 찍어 T1과 픽셀 동일을 확인한다. Covers R1, R11, AC15. Depends on: T2.
- T5. 오버레이 레이어와 툴팁을 만든다: 앵커 프리퍼런스 기반 윈도우 오버레이, 툴팁 컨트롤러와 테스트, 메인 셸 `.help()` 28곳 이관, 힌트 칩 배치, reduced-motion, accessibilityHelp 동등성. Covers R4, R7, R9, R12, R15, AC4, AC5, AC6, AC7, AC8, AC20. Depends on: T3, T4.
- T6. 재설계한다: Orca 방향으로 토큰 값과 컴포넌트 외형을 갱신하고 section 3의 모든 표면과 비정상 상태에 적용한다. Covers R1, R3, AC12. Depends on: T5.
- T7. DESIGN.md를 갱신한다: in-product 컴포넌트 섹션, Don't, Known Gaps, `属于:` 키 교체, lint 0/0, DESIGN.md 토큰 값과 `HideTheme` 값을 비교하는 테스트. AGENTS.md Design Reference를 갱신한다. Covers R2, R5, R14, AC13, AC17, AC18. Depends on: T6.
- T8. 검사 스크립트와 invariant를 만든다: `scripts/check-hide-theme-literals.sh`, 양성 fixture 테스트, `rules add`로 `INV-hide-theme-literals` 등록(`check --bookkeeping`으로 선언 후 작업), CI 워크플로 호출 추가. Covers R11, AC10, AC11. Depends on: T4.
- T9. 최종 증거를 등록하고 검증한다: AC3, AC5, AC12, AC14, AC15, AC17의 스크린샷과 diff, AC19의 완성 트리 인벤토리 비교를 `agents/runs/hide-design-system/`에 두고 등록한 뒤 `sasu implement verify`를 codex judge 쿼터 리셋 뒤에 실행한다. Covers AC16, AC19. Depends on: T7, T8.

## 9. Verification Contract

### 9.1 Test Mode Contract

| Mode | Required For Done | Covers | Human Decision |
| --- | --- | --- | --- |
| build/static | yes | 셸과 core 빌드, 기존 계약 검사, 범위 내 모든 뷰 파일의 리터럴 0건, 컴포넌트 분리와 사용 관계의 정적 검사, 문구·식별자 인벤토리 비교, DESIGN.md lint, invariant 등록 | none |
| automated behavior | yes | 노출 집합, 홀드 상태기계(스냅샷 재계산과 앵커 소멸 포함), 툴팁 컨트롤러, 레지스트리 동등성, help 동등성, 키 통과, reduced-motion, DESIGN.md와 HideTheme 토큰 값 일치, 기존 셸 상호작용 테스트의 회귀 | none |
| app runtime | yes | SC1-SC3을 격리된 dev 인스턴스에서 몰고 창을 찍은 증거; 판정은 codex judge 쿼터 리셋(2026-09-07) 뒤 | none; 사용자는 완료 후 피드백 |

### 9.2 Required Agent Verification

| ID | Mode | Covers | Pass Intent | Required For Done | Can Be Blocked |
| --- | --- | --- | --- | --- | --- |
| V1 | build/static | AC10, AC11, AC13, AC16, AC19, AC20 | 셸과 core가 빌드되고 기존 검사가 통과하며, 리터럴 검사가 `Pet*.swift`와 토큰 파일을 뺀 모든 메인 셸 뷰 파일에서 0건이라 section 3의 모든 표면과 비정상 상태가 토큰으로만 그려짐을 정적으로 보이고, 컴포넌트 분리 검사가 요구 컴포넌트 다섯 종이 각자 파일에 있고 `HideUI.swift`와 뷰 파일이 그것을 정의하지 않고 조립만 하며 이관된 툴팁 사이트 전부가 컴포넌트를 참조함을 보이고, 두 검사 모두 양성 fixture에서 실패하며, 기준선 대비 문구·접근성 식별자 인벤토리 차이가 `.help()` 이관 외에 0건이고, DESIGN.md lint가 0/0이고 invariant가 ledger에 있다 | yes | no |
| V2 | automated behavior | AC1, AC2, AC4, AC6, AC7, AC8, AC9, AC18, AC19 | 셸 테스트가 노출 집합, 상태기계(시트 닫힘 뒤 modifier 재읽기, 포커스 pane·활성 탭·탭 순서 변경 시 대상 교체, pane·탭 닫힘 시 칩 소멸 포함), 툴팁 컨트롤러, 레지스트리와 help 동등성, 키 통과, reduced-motion, DESIGN.md 토큰 값과 `HideTheme` 값의 전수 일치를 증명하고 기존 셸 상호작용 테스트가 모두 통과한다 | yes | no |
| V3 | app runtime | AC3, AC5, SC1, SC2 | 격리된 dev 인스턴스에서 네 조합의 홀드와 hover가 각 카드의 primary path, failure state, recovery를 보이고, 홀드 중 탭을 닫거나 포커스 pane을 바꾼 프레임에서 칩이 사라지거나 옮겨간 것이 캡처와 진단에 담긴다 | yes | no |
| V4 | app runtime | AC12, AC14, AC15, AC17, SC3 | 대표 상태 세트가 DESIGN.md in-product 섹션과 일치하고 Orca 방향이며, 토큰 추출 단계는 픽셀 동일하고, Pet 창과 메뉴바 대시보드는 불변이고, AGENTS.md가 R14를 담는다 | yes | no |

### 9.3 Human Verification

None required within the run.
D-16에 따라 시각 결과의 취향 판단은 완료 후 사용자 피드백으로 받고 후속 라운드가 흡수한다.

## 10. Risks And Open Decisions

- 판정형 심사 타이밍: V3, V4의 judged AC는 codex 백엔드가 필요하고 쿼터는 2026-09-07에 리셋된다. 기계 AC와 증거 등록을 먼저 끝내고 verify는 리셋 뒤에 한 번 실행한다. 이는 park가 아니라 대기다.
- 회귀와 의도된 변화의 혼합(D-09): T4 끝의 픽셀 동일 세트(AC15)가 경계다. T4 안에서 픽셀이 바뀌면 회귀로 다룬다.
- 취향 불일치: A2-A5의 값이 사용자 취향과 다를 수 있다. 실패가 아니라 후속 라운드 입력이다(D-16).
- 성능: 오버레이는 hover와 홀드 시에만 갱신되어야 하고, 노출 집합 재계산은 스냅샷 변경과 modifier 변경에만 일어나야 한다. 매 tick 재계산이나 mutex 아래 작업을 더하지 않는다(Performance Guide).
- 일반 chord 번쩍임: 150ms 지연으로 막지만 아주 느린 ⌘K에는 힌트가 잠깐 보일 수 있다. 사용자가 이미 수용한 현행 동작이다.
- 합성 modifier 홀드: AC3의 스크립트 홀드는 CGEvent로 만든다. 합성이 불가능하면 실제 홀드 캡처로 대체하고 그 사실을 증거에 적는다.
- 워크트리 기준: A1의 로컬 main이 원격보다 뒤일 수 있다.

## 11. Implementation Guardrails

- 원칙: engineering 1(구식이 된 keycap 렌더링과 하드코딩 chord를 같은 변경에서 삭제), 2, 5(토큰·컴포넌트·뷰 분리), 7(`ShellMenuCommand`, `PaneShortcut`, `LaunchArguments`, 기존 hover 패턴 활용), 13(리터럴 검사가 클래스 수정); design 5(기존 패턴 확장), 7(상태를 시각으로 부호화).
- 디자인: 새 색, 반경, 여백, 글꼴 크기는 `HideTheme`에만 추가한다. DESIGN.md 없이 값을 정하지 않는다. 그림자 없음, hairline 1px, Inter ss03, 채도 높은 색은 chrome에 쓰지 않는다.
- 증거는 `agents/runs/hide-design-system/` 아래에만 둔다. `docs/screenshots/`, `docs/verification/`은 쓰지 않는다.
- dev 인스턴스: 워크트리의 `macos/scripts/build_dev_app.sh`가 별도 번들 식별자를 준다. 스크린샷 전에 `HERDR_SOCKET_PATH`로 격리된 herdr 서버를 쓰고 인스턴스가 정확히 하나임을 확인한다(FACT-dev-runtime-instances, incident 2026-09-06).
- Check 바인딩은 env 접두나 `&&` 없는 단일 argv이고 워크트리 밖으로 나가지 않는다. `/tmp` 빌드 디렉터리는 허용된다.
- `agents/rules/invariants/INV-hide-theme-literals.md`와 `INDEX.md` 변경은 `check --bookkeeping`으로 먼저 선언하고 `rules add`로만 쓴다. ledger를 손으로 고치지 않는다.
- herdr-core의 런타임 동작, C ABI, snapshot 포맷, `PetTheme`, `Pet*.swift`, 컨텍스트 메뉴는 건드리지 않는다. herdr-core에서는 `herdr-core/src/runtime.rs`의 `#[cfg(test)] mod tests` 읽기 경로 6줄만 허용되며 이는 테스트 전용 변경이고 런타임 코드는 바꾸지 않는다. `scripts/check-capability-readers-off-lock.sh`는 origin/main 2cb94d7의 본문을 그대로 가져온 것 외에 바꾸지 않는다.
- 로컬 키 모니터는 flagsChanged 관찰 외에 어떤 이벤트도 새로 소비하지 않는다.
- 기존 접근성 식별자(`hide-*`)와 라벨은 유지한다. 문구는 바꾸지 않는다.
- 커밋, 브랜치, PR 텍스트에 에이전트·모델·도구 이름을 넣지 않는다.

## 12. Implementation Result Report Contract

보고서 맨 위에 "사용자 결정을 대신한 가정" 목록(A1-A10과 D-34)을 둔다.
그 다음:

- 상태: Done, Partially Done, Blocked.
- 사용자 가시 변경: 힌트 매트릭스 실제 적용 목록, 툴팁 이관 28곳, 재설계된 표면 목록, 토큰 값 변경 목록.
- 구조 적합성: 새 파일 목록, 삭제된 렌더링(`SidebarBadge` keycap 용도, `HideSettingsKeycaps`, 인라인 keycap, 하드코딩 chord 7곳), 오버레이 레이어 위치, `HideTheme` 추가 토큰.
- AC별 상태와 V별 증거 경로(`agents/runs/hide-design-system/` 아래), judged AC의 판정 시각과 백엔드.
- 검증: 기계 검사 출력 요약, 리터럴 검사 양성·음성 결과, DESIGN.md lint 결과, 판정형 심사 결과, 기계 실패 시 judge 호출 0회 증거, finalize의 실행 호출 0회 증거.
- 편차: 손 dispatch, 판정 대기, park, 재바인딩 등 계약에서 벗어난 모든 것.
- 후속 항목: (1) 기록 트리의 `agents/rules/` 변경을 기본 체크아웃 `~/projects/herdr-ide/agents/rules/`로 복사, (2) PRD를 `git add -f agents/prd/hide-design-system/prd.md`로 병합 브랜치에 커밋, (3) 사용자 취향 피드백 라운드.
- 배포 결과: 워크트리 브랜치와 로컬 커밋 해시. push와 PR 없음.
