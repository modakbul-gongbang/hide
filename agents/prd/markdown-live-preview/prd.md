---
topic: "Markdown Live Preview: Obsidian식 결합 편집 뷰"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "에디터 표면과 코어의 탭별 마크다운 모드 필드가 바뀌는 사용자 표면 변경이며, 자격 증명·프로덕션 데이터·파괴적 효과는 없다. 자동저장 경로는 그대로 쓴다."
source_intake: "current conversation"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-19"
updated_at: "2026-09-19"  # amended: list editing rules (D-14, B16-B22)
---

# PRD: Markdown Live Preview: Obsidian식 결합 편집 뷰

## Goal

hide에서 Markdown 파일을 여는 개발자가 Obsidian의 Live Preview처럼 한 뷰에서 타이핑하면서 서식이 바로 보이게 한다.
커서가 없는 줄은 마크업 기호가 숨겨진 채 헤딩·강조·코드·리스트·인용·링크가 서식으로 보이고, 커서가 놓인 줄만 원문 마크업이 드러난다.
지금은 읽기 전용 Preview와 소스 Edit이 별개 뷰라서 서식을 보려면 편집을 멈춰야 한다.
macOS 14·MIT 호환 네이티브 라이브러리가 없다는 조사 결과(Marklet은 GPL, v57/Markdown은 라이선스 없음·macOS 27, CodeMirror 계열은 WebView 필요)에 따라 Marklet·v57이 쓰는 TextKit 1 레이아웃 매니저 패턴을 참조 설계로 직접 구현한다.

## Non-goals

- 표·이미지·HTML 블록·각주·체크박스 리스트는 Live 뷰에서 서식으로 그리지 않고(체크박스 `- [ ]`의 Enter 이어쓰기도 없다) 모노스페이스 원문으로 보인다 (D-03). 사용자는 그 부분을 소스로 읽는다. 표나 체크박스 위젯 요청이 들어오면 다시 연다.
- 읽기 전용 렌더 Preview 모드는 없앤다 (D-02). 링크 클릭·표 셀 구분 읽기 뷰는 Live 뷰가 대신한다. Obsidian의 Reading view 같은 세 번째 모드 요청이 들어오면 다시 연다.
- 마크업 숨김에 애니메이션은 없다 (D-04). 커서 이동 시 줄 폭이 즉시 바뀐다.
- Live 뷰에 줄 번호 룰러와 Wrap 토글은 없다 (D-05). 소스 모드는 지금 그대로다.
- 256 KB를 넘는 Markdown은 Live 뷰를 제공하지 않는다 (D-07). 사용자는 소스로 편집한다. 성능 측정으로 여유가 확인되면 다시 연다.
- `MarkdownPreview.swift`와 `MarkdownDocument.render`, 그 테스트, "Empty document / Choose Edit" 상태는 같은 변경에서 제거한다 (engineering/principles.md rule 1).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Live 뷰는 편집 가능한 NSTextView 하나 위에 구현한다: 소스 텍스트가 그대로 텍스트 스토리지이고, 서식은 속성으로, 마크업 숨김은 `NSLayoutManager` 서브클래스가 해당 글리프를 그리지 않는 방식(Marklet·v57 패턴)으로 한다. 텍스트 스토리지에는 항상 원문이 있어 draft·자동저장·Find·선택 복사가 소스 모드와 같은 경로를 쓴다. WebView·CodeMirror는 채택하지 않는다. | 사용자 "Obsidian 방식으로 하고"; 조사 결과(GPL·라이선스 없음·WebView) 와 Observer 권장 수락; engineering rule 6 |
| D-02 | 마크다운 모드는 Live(기본)와 Source 둘이다. 문서 툴바의 HideChoiceGroup이 "Live / Source"가 되고, 코어의 탭별 `markdown_preview` 필드는 `markdown_live`로 이름을 바꿔 같은 수명(탭별, 재오픈·재시작 시 기본 Live)을 유지한다. 읽기 전용 Preview 뷰는 삭제한다. | 사용자 "View,Edit이 같이결합된 식으로"; 두 모드는 가정(engineering rule 2) |
| D-03 | 1차 범위의 서식: 헤딩 1~6(크기·굵기, `#` 숨김), 굵게·기울임·취소선(기호 숨김), 인라인 코드(모노·배경, 백틱 숨김), 펜스 코드 블록(모노·배경, 펜스 줄은 커서가 블록 안에 없을 때 숨김), 순서 없는·있는 리스트(마커를 `•`/숫자로 그리고 들여쓰기), 인용(들여쓰기·왼쪽 규칙선, `>` 숨김), 링크(텍스트만, `[]()`와 URL 숨김), 수평선. 나머지 문법은 원문 그대로 모노스페이스. | 가정: Obsidian Live Preview의 기본 집합에서 위젯이 필요한 것을 뺐다 |
| D-04 | 마크업이 드러나는 줄은 커서(삽입점)가 있는 줄과 선택 범위가 걸친 모든 줄이다. 커서가 떠나면 즉시 숨긴다. 펜스 코드 블록은 블록 전체가 한 단위다. | Obsidian의 동작을 따른다(가정) |
| D-05 | Live 뷰는 항상 줄바꿈하고 기존 Preview의 720pt 읽기 폭(`HideTheme.Editor.documentWidth`)을 쓰며 본문 글꼴은 기존 Preview의 Inter 15pt·5pt 행간, 코드는 기존 에디터 모노스페이스 글꼴이다. 텍스트 스케일 chord는 두 모드 모두에 적용된다. 줄 번호 룰러·Wrap 토글은 Source에만 있다. | 가정: 기존 Preview 타이포 토큰 재사용(engineering rule 7, design rule 5) |
| D-06 | 파서는 Foundation의 `AttributedString(markdown:)`에 소스 위치 옵션을 켜서 얻는 블록·인라인 구조와 소스 범위를 쓴다. 이것이 D-03 항목 중 하나라도 정확한 소스 범위를 주지 못하면 `apple/swift-markdown`(Apache-2.0)을 `macos/Vendor`에 기존 방식으로 들여오고, 어느 항목 때문인지 최종 보고에 적는다. | engineering rule 7(있는 것 먼저), 6(있는 라이브러리 채택) |
| D-07 | 재파싱은 편집마다 하되 한 런루프 턴에 한 번으로 합치고, 256 KB를 넘는 파일은 Live를 끄고 Source로 열며 툴바 옆 notice "Live preview is off for files over 256 KB"를 보인다. 키 입력당 추가 작업은 파싱 O(n)과 속성 재적용이며 `Mutex<Runtime>`은 관여하지 않는다. | engineering rule 15; docs/PERFORMANCE_TESTING.md의 입력당 작업 설명 의무 |
| D-08 | 링크는 Command-클릭으로 열고 일반 클릭은 커서를 놓는다. 열기 규칙은 기존 `openDocumentLink`(http/https는 외부 브라우저, 체크아웃 안 상대 파일은 Explorer reveal, 그 외 notice)를 그대로 쓴다. | 편집 뷰에서 일반 클릭은 편집이어야 하므로(가정); DESIGN.md 기존 링크 규칙 |
| D-09 | 빈 문서는 Live 모드에서 빈 편집 뷰와 커서로 열린다. 파싱 실패는 그 턴의 서식 적용을 건너뛰고 원문을 모노스페이스로 보이며 notice에 사유가 있다. 읽기 전용·충돌 배너는 지금 위치 그대로다. | engineering rule 4; design rule 9 |
| D-10 | 자동저장·draft 에코 버퍼·탭 전환·닫기 시 저장 의도 전달은 소스 모드의 기존 경로를 바꾸지 않고 그대로 쓴다. 서식 적용은 스토리지의 문자열을 바꾸지 않는다. | DESIGN.md의 기존 draft/autosave 계약 |
| D-11 | `design/hide.pen`에 `Screen / Editor / Markdown live`(서식 줄, 커서 줄 마크업 노출, 코드 블록, 빈 문서, 256 KB 초과 notice) 보드를 그리고, `DESIGN.md`의 File document toolbar and Markdown 문단을 새 두 모드에 맞게 다시 쓴다. | AGENTS.md 디자인 캔버스 규칙 |
| D-12 | 원칙 반영: engineering/principles.md와 design/principles.md(oh-my-principle fa5186d)를 전부 읽었다. rule 1은 Preview 뷰 삭제로, rule 6·7은 D-01·D-06으로, rule 15는 D-07로, design rule 3은 편집 중 서식 확인에 모드 전환이 필요 없는 것으로, design rule 9는 B9~B12로 번역했다. design rule 11(구조적으로 다른 후보 제시)은 사용자가 Obsidian 방식을 이미 지정해 적용하지 않았다. | sasu principles list |
| D-14 | 리스트 편집 규칙을 orca(stablyai/orca, MIT)의 Tiptap 에디터 동작에서 그대로 옮긴다. 구현은 코드가 아니라 규칙을 옮기는 것이며(orca는 ProseMirror 문서 모델, hide는 소스 텍스트), NSTextView의 `insertNewline`·`insertTab`·`insertBacktab`·`deleteBackward`를 가로채 현재 줄의 마커를 읽고 텍스트를 넣는다. 규칙: (1) 줄 머리에 `- `, `* `, `1. `을 치면 그 줄은 리스트 항목이다(소스 기반이므로 별도 변환 없음, 커서가 떠나면 서식으로 보인다); (2) 항목 안에서 Enter는 같은 들여쓰기·같은 마커의 새 항목이고 번호 리스트는 다음 번호다; (3) 빈 항목에서 Enter는 마커를 지우고 리스트를 빠져나온다; (4) Tab은 항목을 한 단계 들여쓰고 Shift-Tab은 내어쓰며, 중첩 번호 리스트는 자기 열에서 1부터 센다; (5) 빈 항목에서 Backspace는 마커만 지운다; (6) `1. `만 있는 줄에서 Enter는 리스트 탈출이 아니라 글자 그대로 둔다(orca의 모호성 처리); (7) 같은 열의 번호는 Enter·Tab·Shift-Tab·Backspace 뒤에 1부터 다시 매긴다; (8) IME 조합 중(`hasMarkedText`)에는 (2)~(7)을 전부 건너뛴다. 두 모드(Live, Source) 모두에 적용한다. | 사용자 "ㄱㄱ 저거 잘 orca꺼 베껴서 넣어보는거 추가해서 안내해" (2026-09-19); 규칙 출처 orca `rich-markdown-list-continuation.ts`, `rich-markdown-list-indent.ts`, `rich-markdown-key-handler.ts` |
| D-13 | 배포: `agents/config.json`대로 run worktree에서 구현하고 PR로 전달하며 CI를 지켜본다. | agents/config.json delivery.mode=pr |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Markdown 파일을 열면 Live 모드로 열리고, 툴바 가운데에 "Live / Source" 선택이 보인다. 커서가 없는 줄에서 `# 제목`은 `#` 없이 큰 헤딩으로, `**굵게**`는 기호 없이 굵게, `` `code` ``는 백틱 없이 모노·배경으로, `[텍스트](url)`은 텍스트만 링크 색으로 보인다. | D-01, D-02, D-03 |
| B2 | 어떤 줄을 클릭하거나 방향키로 커서를 옮기면 그 줄의 마크업 기호가 원문 그대로 나타나고, 커서가 떠나면 다시 숨겨진다. 여러 줄을 선택하면 걸친 줄 전부 원문이 보인다. | D-04 |
| B3 | 서식이 보이는 줄에서 타이핑하면 글자가 커서 위치에 정확히 들어가고, 소스 모드로 바꾸면 방금 친 원문이 그대로 있다. 자동저장·"Unsaved" 표시·충돌 배너는 소스 모드와 같은 시점에 같은 내용으로 나온다. | D-01, D-10 |
| B4 | 펜스 코드 블록은 커서가 블록 밖에 있으면 펜스 줄이 숨겨지고 본문만 모노·배경으로 보이며, 커서가 블록 안에 들어오면 펜스 줄이 나타난다. | D-03, D-04 |
| B5 | 리스트는 `-`가 `•`로, `1.`은 숫자로 들여쓰기와 함께 보이고, 인용은 왼쪽 규칙선과 들여쓰기로, `---`는 수평선으로 보인다. | D-03 |
| B6 | 표·이미지·HTML·각주·체크박스는 모노스페이스 원문으로 보이고 편집할 수 있다. | D-03 |
| B7 | 링크를 Command-클릭하면 http(s)는 외부 브라우저로, 체크아웃 안 상대 파일은 Explorer에서 reveal되며, 그 외는 기존 notice가 보인다. 일반 클릭은 커서만 놓는다. | D-08 |
| B8 | Live 뷰는 720pt 폭에서 줄바꿈되고 줄 번호가 없다. Source로 바꾸면 지금의 룰러·Wrap 토글·모노스페이스가 보인다. 텍스트 스케일 chord는 두 모드에서 모두 글자 크기를 바꾼다. | D-05 |
| B9 | 빈 Markdown 파일은 Live 모드의 빈 편집 뷰로 열려 바로 타이핑할 수 있다. "Empty document" 상태는 더 이상 없다. | D-09 |
| B10 | 256 KB를 넘는 Markdown은 Source로 열리고 notice "Live preview is off for files over 256 KB"가 보이며 Live 선택은 비활성이다. | D-07 |
| B11 | 모드 선택은 탭별로 기억되어 다른 탭에 갔다 와도 유지되고, 탭을 닫았다 다시 열거나 앱을 재시작하면 Live로 시작한다. | D-02 |
| B12 | 파싱이 실패한 턴에는 원문이 모노스페이스로 보이고 notice에 사유가 있으며 입력은 계속된다. | D-09 |
| B13 | 10,000줄 이하 문서에서 타이핑·커서 이동에 눈에 띄는 지연이 없다: 키 입력당 파싱과 속성 적용 시간을 측정해 `docs/PERFORMANCE_TESTING.md`의 형식(idle·driven 분리)으로 run 디렉터리에 기록한다. | D-07 |
| B14 | 코어 스냅샷의 탭 필드가 `markdown_live`이고, 이전 `markdown_preview`를 읽는 코드와 `MarkdownPreview.swift`·`MarkdownDocumentTests`·Preview 전용 문구가 트리에 없다. | D-02 |
| B15 | `design/hide.pen`에 D-11 보드가 있고 `node scripts/check-design-contract.mjs`가 통과하며 `DESIGN.md` 문단이 두 모드를 설명한다. | D-11 |
| B16 | 리스트 항목 끝에서 Enter를 치면 다음 줄이 같은 들여쓰기의 `- ` 항목으로 시작하고, 번호 리스트면 다음 번호(`3. `)로 시작하며 커서가 마커 뒤에 놓인다. | D-14 |
| B17 | 마커만 남은 빈 항목에서 Enter를 치면 마커가 사라지고 빈 일반 줄이 되며, 번호 리스트였으면 그 아래 남은 항목의 번호가 1부터 다시 매겨진다. | D-14 |
| B18 | 항목에서 Tab을 치면 한 단계(공백 2칸) 들여써지고 번호 항목은 중첩 열의 `1.`이 되며, Shift-Tab은 한 단계 내어쓰고 원래 열의 번호로 돌아간다. 리스트 밖에서 Tab은 지금처럼 탭 문자다. | D-14 |
| B19 | 마커만 남은 빈 항목에서 Backspace를 치면 마커만 사라지고 줄은 남는다. 글자가 있는 항목에서는 일반 Backspace다. | D-14 |
| B20 | `1. `만 친 줄에서 Enter를 치면 그 줄은 글자 그대로 남고 다음 줄은 일반 줄이다. | D-14 |
| B21 | 한글을 조합하는 중에는 Enter·Tab·Backspace가 조합기의 동작을 그대로 따르고 리스트 규칙이 끼어들지 않는다. 조합이 끝난 뒤의 Enter부터 B16이 적용된다. | D-14 |
| B22 | B16~B21은 Source 모드에서도 같이 동작하고, 그 결과 파일 내용은 사용자가 손으로 친 것과 같은 평범한 마크다운이다(특수 문자·숨은 마크업 없음). | D-14 |

## Technical structure

- `macos/Sources/HerdrMacOS/`: Live 뷰는 `HighlightedCodeEditor`와 같은 TextKit 1 구성(NSTextStorage → 커스텀 NSLayoutManager → NSTextContainer → NSTextView)이며, 마크다운 구조를 소스 범위로 돌려주는 파서 어댑터와 커서 줄에 따라 숨김 범위를 계산하는 순수 함수(테스트가 값으로 묻는다)가 분리된다. 새 색·크기는 `HideTheme`에서만 온다.
- `herdr-core`: 탭 스냅샷 필드 이름 변경(`markdown_live`)과 그 이벤트 페이로드. C ABI 시그니처는 그대로다.
- 리스트 편집 규칙(D-14)은 마커 파싱과 다음 줄 텍스트 계산을 순수 함수로 두어 테스트가 값으로 묻고, NSTextView 서브클래스는 그 결과를 넣기만 한다. Live·Source 두 뷰가 같은 함수를 쓴다.
- 파서: Foundation 우선, 부족하면 `apple/swift-markdown`을 `macos/Vendor`에 path 패키지로(D-06).
- 다른 서비스·스키마·인프라 변경 없음.

## Risks

- 글리프 숨김과 캐럿 위치: 숨긴 글리프 위에 커서가 놓이면 캐럿이 보이지 않거나 선택 하이라이트가 어긋날 수 있다. 바운드는 D-04(커서 줄은 항상 원문)와 실제 AppKit 레이아웃 테스트(기존 native editor 테스트 패턴).
- 한글 입력(IME 조합 중) 서식 재적용이 조합을 끊을 수 있다. 바운드는 `hasMarkedText` 동안 속성 재적용을 미루는 것. 검증은 한글 문장 타이핑 관찰(design rule 12).
- Foundation 파서의 소스 위치가 일부 노드에서 비어 있을 수 있다. 바운드는 D-06의 대체 경로.
- 큰 문서 성능: B13의 측정이 기준이며 임계는 기록으로 남기고 acceptance는 "눈에 띄는 지연 없음"이다.
- 라이브 증명 경계: 로컬 파일 편집만이며 네트워크·운영자 pane 조작이 없다. 스크린샷과 측정은 `agents/runs/<slug>/`에만 둔다.
- 사용자가 미리 할 일: 없음.
- 열린 결정: 없음. 가정으로 표시한 D-02~D-05·D-08은 되돌릴 수 있는 구현 선택이며 최종 보고에 나열한다.
