---
topic: "Web shell design system reset: shadcn + Tailwind + Pen"
status: "ready"
human_approval: "approved"  # user 2026-09-25 verbatim: ㅇㅇㅇㅇ 승인하고~ 작업 opus 5.5로 implemnet 가즈아~
review_profile: "standard"
review_rationale: "Every web screen's controls, tokens and theme change and the core's persisted UI state gains a theme field, but no auth, external side effect or production-data migration is involved."
source_intake: "agents/interview/web-design-system-reset/qa-log.md"
created_at: "2026-09-26"
updated_at: "2026-09-26"
---

# PRD: Web shell design system reset: shadcn + Tailwind + Pen

## Goal

hide를 쓰는 운영자는 지금 web shell에서 화면마다 손으로 짠 버튼, 필드, 메뉴, 모달, 팔레트를 만나고, 그 품질과 동작이 화면마다 다르다.
이 변경은 web shell의 UI 계층을 shadcn/Radix 부품과 Tailwind v4 위에 다시 세우고, 같은 부품을 Pen 라이브러리의 `System /` 시트로 1:1 그려 Pen과 코드가 한 기준을 공유하게 하며, 라이트와 다크 테마를 지원하고 DESIGN.md를 은퇴시킨다.
기존 화면의 모든 기본 컨트롤이 새 부품으로 바뀌고, Pen에는 정리된 부품 라이브러리와 현재 web 화면 전체가 기록되어, 이어지는 화면별 리디자인 PRD가 이 기반에서 바로 시작할 수 있게 된다.

## Non-goals

- 화면 레이아웃과 `Component /` 조합의 시각적 리디자인(사이드바, 탭, Settings, Sessions 등)은 하지 않는다. 운영자는 이번에 배치가 같은 화면을 새 부품, 새 밀도, 테마로 본다. 영역별 후속 PRD가 Pen 후보 비교로 진행한다(D-06).
- Storybook은 도입하지 않는다. 부품 확인은 dev 전용 `/gallery`가 맡는다. 재검토: gallery로 부족한 인터랙션 테스트 요구가 생길 때(D-05).
- `Component /`와 화면은 gallery에 넣지 않는다. 재검토: 리디자인 PRD에서 실제 앱 상태 재현 비용이 과하다고 확인될 때(D-04).
- Swift shell(`macos/`)의 모양과 `HideTheme.swift` 값은 바꾸지 않는다. S6에서 삭제된다(D-08).
- Pen export와 코드 렌더의 픽셀 일치 자동 판정은 하지 않는다. Pen이 글꼴을 대체하므로 사람이 판단한다(D-12).
- 승인된 화면 시안 외의 scratch, 캡처, 비교 이미지는 커밋하지 않는다(AGENTS.md Evidence 규칙).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | DESIGN.md를 삭제한다. 시각 기준은 Pen 라이브러리, 수치 기준은 `design/tokens.json`, 코드 기준은 `web/src/components/ui`의 shadcn 부품이다. DESIGN.md의 동작 규칙(Web Workspace, Web Project Sessions, 터미널 첨부 경계, Project Home 등)은 담당 `docs/` 가이드로 옮기고 Swift 픽셀 스펙은 버린다. Swift 동결(C안)과 대체 문서 없는 완전 삭제(B안)는 기각. | Q1 "A로 가자" |
| D-02 | Pen 라이브러리 기반은 Pen 내장 `pencil:shadcn` 라이브러리를 hide 토큰으로 다시 입혀 `design/hide-ui.lib.pen`의 `System /` 시트로 가져온 것이며, 이름과 variant가 shadcn과 1:1이다. 유지되는 `Component /`는 이번에 System master 위에서 재조립하고, 시각 리디자인은 후속 PRD로 둔다. 기존 라이브러리 유지 후 직접 그리기(B), 새 파일 분리(C)는 기각. | Q3 "A로 가자", Q19(Q17 질문) "A로 가자" |
| D-03 | 토큰 의미 계층을 shadcn 이름(background, foreground, card, popover, primary(-foreground), secondary, muted(-foreground), accent, destructive, border, input, ring, sidebar-*)으로 바꾸고 값은 hide 표면 단계를 유지한다. web 클래스는 이번에 일괄 이관한다. hide 이름 유지(B)는 기각. | Q4 "토큰이릉믄 shadcn 체계로" |
| D-04 | dev 전용 `/gallery`가 모든 `System /` 상태를 실제 부품으로 고정 렌더링하고, Pen System master 이름과 gallery 섹션이 어긋나면 check가 실패한다. `Component /`와 화면은 실제 앱 Playwright 캡처를 Pen export와 나란히 사람이 판단한다. 캡처는 `agents/runs/`에만 둔다. | Q5 "A로 가자", Q8 "ㅇㅇ 괜찮은데" |
| D-05 | Storybook은 쓰지 않는다. | Q2 "A좋은데" |
| D-06 | 이번 PRD는 기반과 기본 컨트롤 전면 교체까지이고, 화면 레이아웃과 조합 리디자인은 영역별 후속 PRD로 나눈다. | Q6 "B인데 A하고 다음 PRD로 뽑아서하는거로" |
| D-07 | 중간 밀도(본문 약 12~13px, 컨트롤 약 28px)로 올린다. 정확한 스케일은 Pen `System / Foundations`에 그려 사용자가 승인한 뒤 토큰을 확정한다. 현재 밀도 유지(A), shadcn 기본(C)은 기각. | Q9 "중간밀도로 가자 B" |
| D-08 | Swift shell은 동결한다. `gen-tokens.mjs`는 더 이상 `HideTheme.swift`를 생성하거나 수정하지 않고, DESIGN.md에 기대는 macOS 디자인 계약 테스트와 check 스크립트는 제거한다. `tokens.json`은 web 전용이 된다. | Q10 "동결해; 다 지울거임" |
| D-09 | 아이콘은 lucide-react로 통일한다. `web/src/icons.tsx`의 손그림 SVG는 lucide 대응이 있으면 교체 후 삭제하고, AgentMark 같은 hide 고유 표시만 남긴다. | Q12 "rr"(한글 자판 ㄱㄱ, Q10 추천 수락) |
| D-10 | Tailwind v4로 올린다. `@tailwindcss/vite`가 `tailwind.config.js`, postcss, autoprefixer 설정을 대체하고 `gen-tokens.mjs`가 `tokens.css`의 `@theme`을 쓴다. v3.4 유지(B)는 기각. | Q14 "A" |
| D-11 | 강조색 선택(Lime/Sky/Violet/Amber)은 `--primary`, `--ring`, 에디터 커서를 바꾸고, `--primary-foreground`는 모든 강조색 위에서 읽히는 값으로 고정한다. 링과 커서만 바꾸는 안(B)은 기각. | Q13 "A로 가자" |
| D-12 | Pen은 코드 export가 없고 Radix 동작을 표현하지 못하며 글꼴을 대체한다. Pen과 코드 비교는 사람의 시각 판단이다. Pen 내장 shadcn 라이브러리는 lucide 아이콘과 `Mode: Light/Dark` 테마 축을 쓴다. | 사실: `pen --help`, `@pen.dev/cli/dist/out/data/shadcn.lib.pen`, AGENTS.md Design Library |
| D-13 | 라이트 모드를 포함한다. `tokens.json`은 라이트와 다크 값을 갖고 Pen 변수는 `Mode` 테마 축으로 두 값을 가지며, 각 System master는 한 번 그리고 Light/Dark 프레임으로 미리 본다. 라이트 팔레트는 Foundations에서 사용자가 승인한다. 터미널(ANSI 16색 포함)과 에디터 구문 강조도 라이트 테마를 갖는다. 다크만(A)은 기각. | Q15 "B로 하자" |
| D-14 | Settings는 System / Light / Dark를 제공하고 기본값은 Dark다. System은 macOS 외관 변경을 즉시 따른다. 기본값 System(B), 선택지 없음(C)은 기각. | Q16 "a" |
| D-15 | 테마 선택은 core의 UI 상태(`ui_state`)에 글자 크기, 강조색과 같은 방식으로 저장되고 하나의 typed event로 바뀐다. 알 수 없는 저장값은 Dark로 읽고 진단 로그를 남긴다. | 가정: 기존 외관 설정 경로를 따름(`herdr-core/src/model.rs`, AGENTS.md "core owns Hide's UI state") |
| D-16 | 글자 크기 설정(11~17, `--interface-scale`)은 지금 동작과 저장값을 유지하고, 새 중간 밀도가 배율 1이 된다. | 가정: `web/src/App.tsx:104-111` 기존 동작 보존 |
| D-17 | 디자인 계약 check는 계속 리터럴 값을 거부하고, shadcn 토큰 이름으로 갱신되며 Pen System과 gallery 이름 동기화 검사를 더한다. DESIGN.md나 Swift에만 있던 규칙은 삭제한다. | 가정: D-01, D-04, D-08의 귀결 |
| D-18 | 이번 PRD에서 Pen을 정리한다. 디자인 작업 흐름을 `docs/` 가이드로 쓰고, 33개 `Component /`를 분류해 삭제 또는 재조립하며, web 화면 목록과 화면 시안 커밋 위치를 정하고, 현재 web 화면 전부를 새 부품으로 Light/Dark 두 벌 Pen에 기록한다. 별도 PRD(B), 일부만(C)은 기각. | Q19(Q17 질문) "A로 가자" |
| D-19 | 커밋되는 화면 시안은 `design/hide-screens.pen` 한 파일에 영역별 `Screen / <영역>` 시트로 둔다. 병렬 PR은 줄 단위 병합하지 않고, 합치기 전에 main의 파일에 자기 시트만 노드 단위로 옮겨 넣는 transplant 스크립트를 쓴다. 영역별 파일 분리(B)는 기각. | Q20 "A로 가자!", 시트 단위 이식은 사용자 제안 |
| D-20 | `Component /` 분류 기준: 현재 web 화면에 쓰이면 System 위에서 재조립, web에도 전환 계획에도 없는 Swift 전용이면 삭제, 계획에 있으나 아직 web에 없는 화면용이면 그대로 유지. 구현자가 33행 표를 만들고 사용자가 승인한 뒤에만 삭제한다. 인터뷰 중 분류(B)는 기각. | Q21 "A" |
| D-21 | 삭제되는 `Component /`는 라이브러리에서 제거하고, 유지되는 것은 새 토큰 이름으로 변수를 기계적으로 이관한다. | 가정: D-03, D-18의 귀결 |
| D-22 | 증거: System 상태 전부의 gallery 캡처를 Light/Dark로 Pen export와 나란히, 교체된 기존 화면의 전후 실제 앱 캡처를 Dark/Light로, 기존 web e2e와 단위 테스트 통과, 디자인 계약 check 통과, 옛 손구현 컨트롤 코드 부재. 사람 확인은 `agents/runs/`에서 한다. | Q5, Q6, Q8에서 합성(가정: 구성은 agent) |
| D-23 | 구현 순서는 층으로 진행한다: 토큰과 Tailwind v4 → System 부품과 gallery → 기존 화면 교체와 테마 → Pen 정리와 화면 기록. 각 층이 끝날 때 web shell은 동작하는 상태다. | 가정: engineering principle 3 |
| D-24 | 원칙 입력: `~/projects/oh-my-principle` 654485f의 engineering, design 문서 전체를 읽었다. engineering 1, 6, 7은 B8, B20, B21, B22로, design 5, 6, 7, 9, 12는 B9, B10, B11, B12로 옮겼다. design 1~4, 10, 11, 13은 레이아웃과 데이터 표시를 바꾸지 않는 이 PRD에 해당 행동이 없어 옮기지 않았다. | 원칙 intake |
| D-25 | 전달은 `agents/config.json`의 PR 모드(base `main`, CI watch)로 한다. | 가정: 저장소 설정 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 업데이트 후 처음 연 web shell은 Dark 테마이고, 표면 단계와 배치는 전과 같으며, 컨트롤과 글자가 승인된 중간 밀도로 보인다. | D-06, D-07, D-14 |
| B2 | Settings의 외관 항목에서 System / Light / Dark를 고르면 새로고침 없이 shell 전체, 터미널, 에디터가 즉시 그 테마로 바뀐다. | D-13, D-14 |
| B3 | System을 고른 상태에서 macOS 외관을 바꾸면 hide도 즉시 따라 바뀐다. | D-14 |
| B4 | 고른 테마는 재시작 후에도 유지되고, 알 수 없는 저장값은 Dark로 열리며 화면에 경고 없이 진단 로그에만 남는다. | D-14, D-15 |
| B5 | 강조색을 고르면 두 테마 모두에서 주요 버튼 배경, 포커스 링, 에디터 커서가 그 색으로 바뀌고, 주요 버튼 글자는 네 강조색 모두에서 WCAG AA 대비(4.5:1) 이상으로 읽힌다. | D-11, D-13 |
| B6 | 글자 크기 설정은 전과 같은 범위(11~17)와 저장값으로 동작하고, 인터페이스 글자가 그 배율로 커지거나 작아진다. | D-16 |
| B7 | Light 테마에서 모든 표면, 글자, 상태색(에이전트 작업중, PR 상태, diff, 파일 아이콘)이 읽히고, 터미널은 라이트 ANSI 팔레트를, 에디터는 라이트 구문 강조를 쓴다. | D-13 |
| B8 | 기존 모든 화면의 버튼, 텍스트 필드, select, switch, checkbox, 메뉴(드롭다운, 우클릭, 행 메뉴), 다이얼로그와 시트, 팝오버, 툴팁, 명령 팔레트, 토스트가 shadcn System 부품이며, 이를 대체하던 손구현 코드(`controls.tsx`, 화면별 메뉴/모달/팔레트 구현)는 남아 있지 않다. | D-02, D-06 |
| B9 | 교체된 컨트롤은 기존 동작을 유지한다: 단축키, Esc로 닫기, 닫힌 뒤 이전 포커스(터미널 포함)로 복귀, 메뉴와 팔레트의 방향키 이동, disabled 상태, 결과를 이름으로 적은 파괴적 버튼. | D-06, D-24 |
| B10 | 모든 System 부품은 Pen에 그려진 hover, focus-visible, disabled, open/pending 상태를 두 테마에서 보여 준다. | D-02, D-13, D-24 |
| B11 | 새 밀도에서 한글 라벨, 한영 혼합 이름, 긴 경로와 브랜치 이름이 잘리지 않고 정해진 영역 안에서 줄바꿈되거나 말줄임된다. | D-07, D-24 |
| B12 | 아이콘은 lucide이고 hide 고유 표시(AgentMark 등)는 그대로이며, 의미를 가진 아이콘 전용 버튼은 모두 툴팁과 접근성 라벨을 가진다. | D-09, D-24 |
| B13 | 테마를 바꿔도 열린 터미널은 다시 생성되지 않고 내용, 스크롤 위치, 입력 중인 글자가 그대로 남는다. | D-13 |
| B14 | dev 빌드에서 `/gallery`를 열면 모든 System 부품의 Pen 상태가 같은 이름으로 Light/Dark 두 칸에 보이고 직접 조작할 수 있으며, hided가 서빙하는 production 빌드에는 `/gallery`가 없다. | D-04 |
| B15 | 디자인 계약 check는 Pen System master와 gallery 섹션 이름이 어긋나거나 web 소스에 토큰 밖 리터럴 값이 있으면 실패하고, 전달된 트리에서는 통과한다. | D-04, D-17 |
| B16 | Pen 라이브러리에는 shadcn 부품마다 이름과 variant가 1:1인 `System /` 시트가 Light/Dark 프레임과 함께 있고, 변수는 `tokens.json`에서 생성된 `Mode` 테마 축 값을 가진다. | D-02, D-03, D-13 |
| B17 | `System / Foundations`의 중간 밀도 스케일과 라이트 팔레트는 사용자가 Pen에서 승인한 값으로 `tokens.json`에 확정된다. | D-07, D-13 |
| B18 | 33개 `Component /`의 분류표(이름, 사용하는 web 파일, 분류, 근거)가 사용자 승인을 받은 뒤에만 삭제가 반영되고, 유지된 것은 System master 위에서 재조립되어 새 토큰 변수로 그려진다. | D-18, D-20, D-21 |
| B19 | `design/hide-screens.pen`에 web 화면 목록의 영역마다 `Screen / <영역>` 시트가 있고, 각 시트에 현재 배치가 새 부품으로 Light/Dark 두 벌 그려져 있다. | D-18, D-19 |
| B20 | transplant 스크립트는 지정한 `Screen /` 시트만 main의 `hide-screens.pen`에 노드 단위로 교체해 넣고, 원본에 없는 시트 id는 거부하며, 결과 파일은 gen-pen과 디자인 check를 통과한다. | D-19 |
| B21 | DESIGN.md가 없고, 그 동작 규칙은 담당 `docs/` 가이드에 있으며, `docs/`의 디자인 작업 가이드가 scratch → 승인 → 라이브러리/화면 파일 → 코드 → gallery 또는 앱 캡처 → 한 PR 흐름과 transplant 절차를 설명한다. AGENTS.md, CONTRIBUTING.md, README.md, docs/README.md에 DESIGN.md를 가리키는 링크가 남아 있지 않다. | D-01, D-18, D-19 |
| B22 | `HideTheme.swift`는 바이트 단위로 바뀌지 않고, macOS 빌드와 남은 Swift 테스트는 DESIGN.md 없이 통과한다. | D-08 |
| B23 | web 빌드는 Tailwind v4로 이루어지고 `tailwind.config.js`와 postcss 설정 파일이 없으며, web 빌드, lint, 단위 테스트, e2e가 통과한다. | D-10, D-22 |
| B24 | 전달 시점에 System 상태의 gallery-대-Pen 비교와 교체된 화면의 전후 캡처가 두 테마로 `agents/runs/`에 있고 사용자가 확인했으며, 그 파일은 커밋에 포함되지 않는다. | D-04, D-22 |

## Technical structure

- `design/tokens.json`이 shadcn 의미 이름과 색상별 light/dark 값을 갖는다. `gen-tokens.mjs`는 web `tokens.css`(`@theme`, `:root`, `.dark`)만 쓰고 `HideTheme.swift`는 생성 대상에서 빠진다. `gen-pen`은 같은 값을 Pen 변수의 `Mode` 테마 축으로 쓴다.
- web은 Tailwind v4(`@tailwindcss/vite`)와 shadcn 부품(`web/src/components/ui`, Radix, cmdk, lucide-react와 shadcn이 요구하는 유틸 의존성)을 쓴다. `/gallery`는 dev 빌드에서만 포함되는 라우트다.
- core의 persisted `ui_state`와 snapshot에 테마 필드가 추가되고 테마를 바꾸는 typed event가 하나 생긴다. web 생성 타입은 schema에서 다시 만든다.
- Pen: `design/hide-ui.lib.pen`은 `System /`(shadcn 기반)과 정리된 `Component /`로 재구성되고, 새 파일 `design/hide-screens.pen`이 라이브러리를 import해 화면을 담는다. `scripts/`에 시트 transplant 스크립트가 추가된다.
- DESIGN.md를 삭제하고, DESIGN.md와 Swift 전용 디자인 check와 테스트를 제거하며, AGENTS.md Design Reference와 Design Library 절을 새 기준으로 다시 쓴다.

## Risks

- 범위가 크다. 층 순서(D-23)를 지켜 각 층 끝에서 web shell이 동작하게 하고, 층마다 기존 e2e를 돌린다.
- Radix 포털과 포커스 관리가 터미널 포커스 복귀(`restoreFocus`)와 충돌할 수 있다. B9 회귀는 기존 e2e와 교체 화면 전후 확인으로 잡는다.
- 라이트 테마에서 Lime이나 Amber 강조색은 링과 커서 대비가 낮을 수 있다. B5의 대비 기준을 두 테마 모두에 적용하고, 부족하면 테마별 강조색 값을 Foundations 승인 때 함께 정한다.
- 실행 중인 Pen 데스크톱 앱이 `.pen` 파일을 덮어쓸 수 있다. 커밋 전마다 `design/*.pen`을 HEAD와 diff하고, 편집은 AGENTS.md의 CLI headless 절차를 따른다.
- 사용자 승인 지점은 네 곳이다: 중간 밀도 수치와 라이트 팔레트(B17), Component 분류표(B18), 부품과 화면 비교 캡처(B24). 그 외 사용자가 미리 준비할 계정이나 자격 증명은 없다.
- 네이티브 확인은 격리된 Herdr 서버와 dev 빌드 한 인스턴스에서 하고 운영자의 앱과 서버는 건드리지 않는다(AGENTS.md Performance Guide).
