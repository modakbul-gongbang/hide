---
topic: "Projects 사이드바 타이포·밀도 재설계 (E-mid)"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "사용자가 매일 보는 사이드바의 글자·행·너비를 바꾸고 코어 ui_state에 필드 하나를 더하지만, 데이터·인증·외부 효과는 없다."
source_intake: "agents/interview/sidebar-typography/qa-log.md"
created_at: "2026-09-27"
updated_at: "2026-09-27"
---

# PRD: Projects 사이드바 타이포·밀도 재설계 (E-mid)

## Goal

hide 웹 셸의 Projects 사이드바를 보는 조작자가 프로젝트, 체크아웃, 에이전트를 한눈에 구분해 읽을 수 있게 한다.
지금은 Appearance 글자 크기 배율(1.23)이 사이드바 토큰에 곱해지고, 거의 모든 행이 500~600 굵기이며, purpose 없는 체크아웃 행에도 빈 둘째 줄이 생겨서 목록 전체가 크고 뚱뚱하고 헐렁하게 보인다.
Pen 보드 v1~v7에서 고른 E-mid 사다리(프로젝트 13/600 하나만 굵게, 체크아웃·에이전트 12/400, 메타 11)와 행 높이(36 · 32/48 · 28/44)를 적용하고, 둘째 줄은 내용이 있을 때만 그리며, 사이드바는 배율을 따르지 않고, 너비를 220~440px로 끌어 조절할 수 있게 한다.

## Non-goals

- 항상 두 줄 + 둘째 줄 대체 사다리(purpose → PR → issue → git 사실, 또는 접힌 대표 에이전트 이름)는 넣지 않는다. 결과: purpose가 없는 행은 한 줄이라 1줄·2줄이 섞인다. 재검토: 사용자가 섞인 리듬이 거슬린다고 말할 때 (D-15).
- 한글 폰트(Pretendard 등) 번들은 하지 않는다. 결과: 한글은 시스템 폴백 폰트로 그려진다. 재검토: 구현 캡처에서 한글 줄이 라틴보다 여전히 눈에 띄게 굵어 보일 때 (D-14).
- 에이전트 행의 task/progress 규칙(둘째 줄의 request/news/quiet 톤과 표시 조건)은 바꾸지 않는다. 결과: 조용한 진행 문장은 여전히 tooltip에만 있다 (D-08).
- 사이드바 너비를 키보드로 조절하는 경로는 넣지 않는다. 결과: 포인터 드래그와 더블클릭만 있다. 재검토: 접근성 요구가 들어올 때 (D-09).
- 작업 영역 쪽에 새 최소 폭 가드는 두지 않는다. 결과: 사이드바를 넓히면 기존 좁은 창 규칙이 측면 패널을 먼저 접는다 (D-21).
- 사이드바 밖(탭 스트립, 패널 헤더, 문서, Overview)의 배율 적용은 그대로다 (D-06).
- engineering 원칙 12(테스트 가격): 굵기·크기 자체를 단위 테스트로 고정하지 않는다. 캡처 비교와 e2e 기하 규칙이 증명이다 (D-16).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Pen 보드 v1~v7 중 E-mid를 채택한다. A(지금)·B(배율만)·D(제안+배율)·F(인라인 purpose)·G(전부 1줄)·H/I(항상 2줄)·E2(조밀)·comfort·airy는 기각. | 사용자: "E mid 좋다 이거로 해서 작업 바로 please 해줘~ opus5.5" (인터뷰 D-02) |
| D-02 | 글자 사다리: 프로젝트명과 All projects 13px/600이 화면의 유일한 semibold. 체크아웃 이름 12px/400, 포커스된 체크아웃 500. 에이전트 작업 이름 12px/400(선택·주의 상태에서도 400, 밝기로 구분). purpose·progress 줄·나이·경과 시간 11px(경과 시간 mono, 10→11). 섹션 헤더 10px/500 문장 부호("Projects · Recent activity · 14"), 대문자 아님. All projects 개수 12px. 기존 텍스트 토큰(subhead/body/caption/micro)만 쓴다. | 사용자: "C 괜찮은데?", "음 우선 그냥 e로 하자", "E mid 좋다" (인터뷰 D-03) |
| D-03 | 행 높이: 프로젝트 행 36; 체크아웃 행 한 줄 32 / 두 줄 48; 에이전트 행 한 줄 28 / 두 줄 44(첫째 줄 20, 둘째 줄 16, 위아래 4); 체크아웃 둘째 줄 16; 열린 체크아웃 그룹 채움 안쪽 위아래 4. | 사용자: "E-airy와 E comoft 중간으로? 해줄래?", "E mid 좋다" (인터뷰 D-04) |
| D-04 | 체크아웃 행은 purpose가 있을 때만 두 줄이다. 에이전트가 있어도 purpose가 없으면 한 줄이고 마지막 커밋 나이는 첫째 줄의 시간 열에 놓인다. | 사용자: "음 우선 그냥 e로 하자" (인터뷰 D-05) |
| D-05 | Projects/Agents 사이드바는 Appearance 글자 크기(`--interface-scale`)를 따르지 않는다. 사이드바 안의 모든 텍스트 토큰은 배율 1로 렌더되고, 배율은 나머지 인터페이스에만 적용된다. | 사용자: "사이드바는 배율 안 따르게 하고" (인터뷰 D-06) |
| D-06 | 가정(위임): 체크아웃 이름의 경로 접두어(첫 슬래시까지, `prd/` `gen-prd/` `docs/` `fix/`)는 muted, 슬래시 뒤는 foreground. 슬래시가 없는 이름은 그대로. 접두어 길이에 따른 예외는 없다. | 보드 C/E/E-mid에 그린 대로 채택 (인터뷰 D-07) |
| D-07 | 에이전트 행의 task/progress 규칙은 UI_BEHAVIOR 그대로: 첫째 줄 task(상태 마크, 제공자 마크, 안정된 이름, 접힌 부모의 배지, 경과 시간, 부모 chevron), 둘째 줄 progress(요청은 경고색/오류는 붉은색으로 해결될 때까지, 새 소식은 밝게 읽을 때까지, 조용하면 그리지 않음). 자식 행은 한 단(18px) 들여쓰고 muted. | 사용자: "agent item의 경우 task, progress가 각각 어떻게 보여야할지도 정리" → 보드 v4 상태표 채택 (인터뷰 D-08) |
| D-08 | 사이드바 너비를 오른쪽 가장자리 드래그로 조절한다(사용자 결정). 가정(위임): 범위 220~440(기존 `--size-sidebar-min/max`), 기본 292(`--size-sidebar-ideal`), 더블클릭이면 292로 복귀. 이 수치는 되돌리기 쉬운 에이전트 가정이며 사용자가 거부할 수 있다. | 사용자: "왼쪽 사이드바도 너비 조절 가능하게? + max width는 잡아두고 ㅇㅇ 어느정도 너 생각선에서"; 수치는 가정(위임) (인터뷰 D-09) |
| D-09 | 가정(위임): 너비는 전역 UI 상태라 `font_size`·`left_sidebar_visible`과 같은 경로를 쓴다. UiStateSnapshot에 `sidebar_width` 필드, `ui_state_update` 페이로드의 선택 필드, persistence.rs 저장. 셸은 드래그가 끝날 때와 더블클릭 때 한 번 보내고 드래그 중에는 로컬 미리보기만 그린다. 코어는 범위 밖 값을 거부하고 진단을 남긴다(engineering 4). 워크스페이스별 `views_over_share` 모델은 쓰지 않는다. 잡는 폭 `--size-resize-grab` 20px, 보이는 선 `--size-resize-handle` 2px로 패널 구분선과 같은 부품(design 5). | 인터뷰 D-10 (repo: actions.ts:119-126, runtime/events.rs:539-577, persistence.rs:86-87) |
| D-10 | 행 높이는 토큰으로만 바꾸고 공용 토큰은 건드리지 않는다: 프로젝트 행은 `--size-control-regular` 대신 사이드바 전용 토큰(36), 체크아웃 `--size-checkout-row` 36→32, `-detailed` 44→48, 에이전트 첫째 줄은 공용 `--size-control-sm`(24) 대신 전용 토큰 20, 둘째 줄 16, 위아래 4. 소비자 없는 `--size-compact-agent-row-vpad`/`-leading-inset`는 삭제(engineering 1). 값은 전부 design/tokens.json → gen-tokens. | 인터뷰 D-18 (repo: sidebar.tsx:318,501,644,731; sidebar-agent-row.tsx:95,109; check-web-tokens.mjs) |
| D-11 | Pen 라이브러리 Component 시트 네 장(Project Row, Checkout row, Sidebar agent row, Section Header)과 Screen / Projects Sidebar 빌더를 같은 수치로 같은 PR에서 갱신한다. Section Header 마스터(O79KF)의 라벨은 body/600 → micro/500 문장 부호("Projects · Recent activity · 14")로 바꾸고, 스크린 빌더는 로컬 section()을 지우고 그 마스터를 ref로 쓴다(engineering 1, 7). gen-pen/gen-screens/check-design-contract를 통과시킨다. 라이브러리 편집은 스크래치 복사본에서 노드를 만들어 HEAD JSON에 스크립트로 이식하고 HEAD와 구조 diff한다. 개수(nZkan)의 Pen/코드 드리프트는 코드 쪽(body 12)에 맞춘다. | 저장소 규칙 docs/DESIGN_WORKFLOW.md(One PR); 인터뷰 D-11, D-19 |
| D-12 | 배율 미적용이 닿는 기존 검증을 고친다: e2e의 글자 크기 17 시나리오는 "사이드바 글자·행이 배율 1과 같다"로, 리뷰 타깃 projects-sidebar의 scale 조건은 제거(사이드바에 무의미), DESIGN_WORKFLOW의 scale 쿼리 설명은 "사이드바 밖에만 영향"으로. | 인터뷰 D-17 (repo: web/e2e/projects-sidebar.spec.ts:403-432, design/review-targets.json:2-40) |
| D-13 | 저장된 너비는 nav의 인라인 width가 아니라 CSS 변수로 적용한다. e2e 오버플로 헬퍼(sidebar-geometry.mjs:181)가 nav.style.width를 직접 세팅하므로 그 헬퍼와 240px·min 오버플로 검사, 행 높이 동일성 검사는 그대로 동작해야 한다. | 인터뷰 D-21 |
| D-14 | 같은 PR에서 docs/UI_BEHAVIOR.md(체크아웃 행 줄 규칙·나이 위치, 에이전트 행 높이, 사이드바 너비 드래그·저장·복귀, 사이드바는 배율 미적용), docs/ARCHITECTURE.md(배율 적용 범위), docs/DESIGN_WORKFLOW.md(리뷰 타깃 scale, 갤러리 scale 쿼리)를 고친다. | 저장소 규칙 AGENTS.md; 인터뷰 D-22 |
| D-15 | 전달은 PR 하나. 스펙 브랜치 `gen-prd/sidebar-typography`에서 sasu가 만드는 런 워크트리 `prd/sidebar-typography`, base `main`, 병합은 사용자 승인 후. 앞서 제안한 3-PR 분할은 `/please` 한 번 호출로 대체됨. | 사용자: "작업 바로 please 해줘"; agents/config.json delivery.mode=pr (인터뷰 D-12) |
| D-16 | 구현 에이전트는 Claude `claude-opus-5-5`, effort high. | 사용자: "opus5.5" (인터뷰 D-13) |
| D-18 | 단위 테스트 범위: (1) 체크아웃 줄 선택 - purpose 없음+에이전트 있음 → 한 줄, purpose 있음 → 두 줄; (2) 셸의 드래그 clamp - 220 아래·440 위 포인터에서 보내는 값이 경계값; (3) 코어의 범위 밖 `sidebar_width` 거부. 굵기·크기 자체는 테스트하지 않는다(engineering 12). | 인터뷰 D-16 |
| D-17 | 원칙 intake: `~/projects/oh-my-principle` 커밋 654485f의 engineering/principles.md와 design/principles.md를 전부 읽음. design 7·8·12·13과 engineering 1·4·5·7·12를 행/비목표로 옮겼고, design 11(구조적 후보 선택)은 보드 v1~v7로 이미 이행되어 행이 없다. | 인터뷰 D-16, D-20 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Projects 사이드바에서 프로젝트명과 All projects만 13px semibold이고, 체크아웃 이름과 에이전트 작업 이름은 12px 400이다. 포커스된 체크아웃 이름만 500이며, 선택된 에이전트 행은 400에 secondary 채움이다. | D-02, D-07 |
| B2 | purpose, 에이전트 둘째 줄, 마지막 커밋 나이, 경과 시간은 11px이고 나이·경과 시간은 mono다. 섹션 헤더는 10px 500 문장 부호("Projects · Recent activity · 14")다. All projects 개수는 12px다. | D-02 |
| B3 | 프로젝트 행은 36px, 체크아웃 행은 한 줄 32px / 두 줄 48px, 에이전트 행은 한 줄 28px / 두 줄 44px이며, 열린 체크아웃 그룹 채움은 위아래 4px 안쪽 여백을 가진다. | D-03, D-10 |
| B4 | purpose가 없는 체크아웃은 에이전트가 있어도 한 줄이고, 마지막 커밋 나이가 첫째 줄의 시간 열에 배지·chevron과 함께 선다. purpose가 있으면 둘째 줄에 purpose와 나이가 놓인다. 빈 둘째 줄은 어디에도 없다. | D-04, D-18 |
| B5 | 체크아웃 이름의 첫 슬래시까지(`prd/`, `gen-prd/`, `docs/`, `fix/`)는 muted, 나머지는 foreground다. `main`처럼 슬래시 없는 이름과 폴더 이름은 전부 foreground다. | D-06 |
| B6 | 긴 이름은 이름 칸 끝에서 말줄임되고, 모든 행의 시간·배지·chevron은 각각 한 열에 서며 hover·focus로 어떤 행도 움직이거나 크기가 바뀌지 않는다(기존 e2e 규칙 childIndentPx 18, rootsAligned, stableUnderHover, stableUnderFocus, noOverlap, sharedColumns, noSidewaysOverflow 유지). | D-03, D-13 |
| B7 | 한글·라틴 혼합 이름과 긴 이름(갤러리 content=long)이 새 크기·행 높이에서 잘리거나 겹치지 않고, 11px 둘째 줄이 16px 칸 안에서 잘리지 않는다. | D-02, D-03 |
| B8 | Appearance 글자 크기를 11~17 어느 값으로 바꿔도 사이드바의 글자 크기와 행 높이는 그대로다. 사이드바 밖(문서, 뷰어, Overview, 설정)은 지금처럼 배율을 따른다. | D-05, D-12 |
| B9 | 에이전트 행 첫째 줄은 상태 마크, 제공자 마크, 작업 이름, (접힌 부모의) 배지, 경과 시간, (부모의) chevron이고, 둘째 줄은 요청이면 경고색(오류는 붉은색)으로 해결될 때까지, 새 소식이면 밝게 읽을 때까지 보이며, 조용하면 없다. 자식 행은 18px 들여쓰고 muted다. 바뀐 것은 크기·굵기·높이뿐이다. | D-07, D-02, D-03 |
| B10 | 사이드바 오른쪽 가장자리 20px 폭에 포인터를 올리면 col-resize 커서와 2px 선 강조가 보이고, 끌면 사이드바가 220~440px 사이에서 따라오며 경계에서 멈춘다. 놓으면 그 너비가 유지된다. | D-08, D-09, D-18 |
| B11 | 조절한 너비는 hide를 재시작하거나 재접속해도 같다. 가장자리를 더블클릭하면 292px로 돌아가고 그 값도 저장된다. | D-08, D-09 |
| B12 | 사이드바를 넓혀 작업 영역이 좁아지면 기존 좁은 창 규칙이 측면 패널을 먼저 접는다. 사이드바 자체는 220 아래로 내려가지 않는다. | D-08, D-13 |
| B13 | 범위 밖 너비가 코어에 도착하면 코어가 거부하고 진단 로그에 남기며, 화면에는 아무것도 뜨지 않고 사이드바는 마지막 유효 너비를 유지한다(design 13). | D-09, D-18 |
| B14 | 갤러리 scene=projects-sidebar의 dark/light, 너비 220/292/440, content reference/long 캡처가 보드 v7 E-mid export와 같은 사다리·행 높이를 보이고, 실제 앱의 Projects 사이드바 캡처도 같다. | D-01, D-11 |
| B15 | design/hide-ui.lib.pen의 Project Row, Checkout row, Sidebar agent row, Section Header 마스터와 Screen / Projects Sidebar 시트가 같은 수치를 그리고, `node scripts/check-design-contract.mjs`, `scripts/verify-web.sh`, `scripts/verify-cargo.sh`가 통과한다. | D-11 |
| B16 | docs/UI_BEHAVIOR.md, docs/ARCHITECTURE.md, docs/DESIGN_WORKFLOW.md가 새 줄 규칙, 행 높이, 너비 드래그, 배율 범위, 리뷰 타깃 조건을 말한다. | D-14 |

## Technical structure

- 코어: `UiStateSnapshot`에 `sidebar_width`(px) 필드 하나, 기본 292; `ui_state_update` 페이로드의 선택 필드; persistence.rs의 StoredUiState에 저장·복원; 220~440 밖은 거부 + 진단. 다른 구조 변경 없음.
- 웹 셸: 사이드바 nav가 코어의 너비를 CSS 변수로 받고, 오른쪽 가장자리 리사이즈 핸들(패널 구분선과 같은 부품)이 드래그 종료·더블클릭 때 `ui_state_update`를 보낸다. nav 자신에 `--interface-scale: 1`을 두어 안의 텍스트 토큰이 배율을 받지 않게 한다. 행 높이·줄 규칙·굵기는 sidebar.tsx와 sidebar-agent-row.tsx의 클래스와 토큰 변경.
- 토큰: design/tokens.json에 사이드바 전용 행 높이 토큰 추가·값 변경, 소비자 없는 두 토큰 삭제, gen-tokens로 tokens.css 재생성.
- 디자인: hide-ui.lib.pen의 네 Component 마스터(Project Row, Checkout row, Sidebar agent row, Section Header)와 pen-screens.mjs의 Projects Sidebar 빌더 갱신, gen-pen/gen-screens 재생성, review-targets.json의 scale 조건 제거.
- 검증: 기존 e2e 사이드바 기하 규칙과 스펙 유지·갱신, 너비 드래그·복원 e2e, 단위 테스트(줄 선택, 셸 clamp, 코어 범위 거부; D-18), 갤러리·실제 앱 캡처 등록.

## Risks

- 한글 폴백 폰트가 400에서도 라틴보다 굵어 보이면 D-14 재검토. 캡처 비교로 확인한다.
- 11px 둘째 줄의 기본 행간(1.5 → 16.5px)이 16px 칸을 0.5px 넘는다. 칸은 min-height라 잘리지 않지만, 캡처에서 겹침이 보이면 둘째 줄 토큰을 17로 올리는 것이 되돌리기 쉬운 조정이다.
- e2e 헬퍼가 nav.style.width를 직접 쓰므로 너비를 인라인으로 적용하면 오버플로 검사가 깨진다(D-13). 리뷰어는 nav의 너비 적용 방식을 본다.
- 사이드바를 넓히면 측면 패널의 좁은 창 규칙이 더 자주 발동한다. 기존 규칙의 범위이며 새 상태를 그리지 않는다.
- Pen 라이브러리 편집은 pen CLI save가 무관한 노드를 망가뜨린 전례가 있어 HEAD JSON 이식과 구조 diff가 필수다(D-11).
- 실제 앱 캡처는 격리된 hided/Herdr에서만 찍는다. 조작자의 앱·서버·pane은 건드리지 않는다.
- 사용자가 구현 전에 할 일은 없다.
