---
topic: "파일 뷰어 어댑터: PDF 내장 보기, 기본 앱으로 열기, 브라우저 pane으로 열기"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "코어 스냅샷에 문서 종류 필드가 추가되고 셸이 외부 프로세스(macOS 기본 앱, node browser-pane.mjs)를 사용자 요청으로 실행하는 사용자 표면 변경이며, 자격 증명·프로덕션 데이터·파괴적 효과는 없다."
source_intake: "current conversation"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-19"
updated_at: "2026-09-19"
---

# PRD: 파일 뷰어 어댑터: PDF 내장 보기, 기본 앱으로 열기, 브라우저 pane으로 열기

## Goal

hide를 쓰는 개발자가 Explorer에서 PDF를 더블클릭하면 앱 안의 에디터 자리에서 바로 읽을 수 있고, 어떤 파일 행이든 우클릭해 macOS 기본 앱이나 hide의 브라우저 pane에서 열 수 있다.
지금은 PDF가 "Preview only" 빈 화면이고, HTML은 소스로만 열리며, Explorer 메뉴에는 열기 항목이 하나도 없다.
문서 종류를 코어가 판정해 스냅샷에 싣고 셸이 종류별 뷰 하나로 그리는 어댑터 구조로 바꿔, 새 종류를 붙이는 일이 코어 enum 한 줄과 셸 case 한 줄이 되게 한다.

## Non-goals

- HTML을 앱 안에서 WebKit으로 렌더하지 않는다 (D-05). `docs/BROWSER_PANES.md`가 두 번째 브라우저 엔진을 만들지 않기로 했기 때문이며, 브라우저 pane이 그 자리다. 브라우저 pane이 폐기될 때만 다시 연다.
- PDF는 읽기 전용이다. 주석·서명·페이지 편집은 없고 (D-03), 사용자는 기본 앱으로 열기로 편집한다. 편집 요청이 들어오면 다시 연다.
- 브라우저 pane의 프로필을 고르는 설정 UI는 두지 않는다 (D-06). 가장 최근에 쓴 실행 중 프로필이 자동 선택된다. 사용자가 "나중에 고치자"고 했으므로 프로필 선택 요청이 들어오면 다시 연다.
- 저장 시 브라우저 pane 자동 새로고침은 없다 (D-07). 사용자는 pane에서 직접 새로고침한다. 라이브 리로드 요청이 들어오면 다시 연다.
- 원격 체크아웃의 파일에는 세 기능 모두 제공하지 않는다 (D-08). 원격 파일은 지금처럼 읽기 전용 목록이다.
- 브라우저 pane 열기와 기본 앱 열기는 파일 행에만 있고 폴더 행에는 없다 (D-04). 폴더는 기존 Reveal in Finder가 같은 결과를 낸다.
- 셸의 확장자 목록(`isImage`)과 "Binary files are preview-only" 문자열 분기는 같은 변경에서 제거한다 (D-01, engineering/principles.md rule 1).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 코어의 파일 열기(`files.rs`)가 문서 종류 `document_kind`(`text`, `markdown`, `image`, `pdf`, `binary`)를 판정해 에디터 문서 스냅샷에 싣고, 셸의 `editorContent`는 종류→뷰 매핑 하나(switch)로 그린다. 셸이 확장자로 판정하던 이미지 목록은 코어로 옮긴다. `readonly_reason`은 크기·권한 사유에만 남고 종류 판정 문구를 더 이상 겸하지 않는다. | 사용자 "Viewer adapter level에서 이것저것 연결해서 붙일 수 있지 않을까?" 와 Observer 제안("코어 enum 한 줄과 셸 case 한 줄") 수락 |
| D-02 | 종류 판정은 PDF는 내용(`%PDF-` 시그니처), 이미지는 기존 확장자 집합(png, jpg, jpeg, gif, webp, tiff, heic, avif), 마크다운은 기존 `language_for`, 그 외 UTF-8이면 `text`, 아니면 `binary`. PDF·이미지·binary는 `contents_utf8`가 없다. | 가정: 코어는 이미지를 디코드하지 않으므로 확장자 판정을 유지하고, PDF는 확장자 없이도 여는 게 맞다 |
| D-03 | PDF는 `PDFKit.PDFView`로 그린다: 연속 세로 스크롤, 창 폭에 맞춤 자동 스케일, 텍스트 선택 가능, 읽기 전용. 문서 툴바의 Find는 PDFView의 검색이 없으므로 비활성(툴팁 "Find is unavailable for PDF"), Wrap과 Markdown 모드 컨트롤은 숨김, Reveal 두 항목은 유지. 디코드 실패는 사유가 있는 unavailable 상태. | 사용자 "PDFKit.PDFView 이것도 하면 안돼?"; Find 비활성은 가정 |
| D-04 | Explorer 컨텍스트 메뉴에 파일 행 전용 그룹 "Open with Default App", "Open in Browser Pane"을 New Folder 뒤 구분선 다음, Reveal in Finder 앞에 둔다(VS Code가 열기 항목을 reveal 앞에 두는 순서). 폴더 행·빈 영역·원격 트리에는 없다. | 사용자 "Open with default app 좋다 ㅇㅇ", "Open in browser pane 이것도 좋은듯"; 위치는 가정 |
| D-05 | Open with Default App은 기존 `ExternalFileOpener.open`(NSWorkspace)으로 보낸다. 명시적 메뉴 선택이므로 터미널 링크의 실행 파일 reveal 가드는 적용하지 않고 macOS의 기본 앱 결정을 따른다. 실패는 기존 interaction notice로 보인다. HTML 더블클릭은 지금처럼 소스를 연다. | 사용자 "클릭은 소스 좋은데"; 가드 미적용은 가정(그 가드는 링크 감지 오클릭을 막기 위한 것) |
| D-06 | Open in Browser Pane은 셸이 앱 번들의 `browser-pane/browser-pane.mjs`를 `node`로 실행해 `file://` URL을 연다. 프로필은 관리 프로필(live·external- 제외) 중 실행 중(`.state`가 있는)인 것 가운데 `.state` 수정 시각이 가장 최근인 것. `profiles` 서브커맨드가 `running`과 `state_modified_at`을 함께 돌려주고 셸이 고른다. 실행 중 프로필이 없으면 notice "No running chromux profile. Launch one with chromux launch <name>." | 사용자 "우선 가장 최근 profile로 우선하고 나중에 고치게 하자 browser 쪽은" |
| D-07 | pane 배치는 포커스 워크스페이스의 포커스 pane 오른쪽 split, `--no-focus`(호스트 기존 계약). 같은 파일을 다시 열면 `--key`를 파일 경로에서 유도해 기존 pane을 재사용하고 새로 만들지 않는다(engineering rule 11). 파일이 바뀌어도 자동 새로고침은 없다. | 가정: 사용자가 연 것이므로 옆에 보이면 충분하고 호스트 계약을 바꾸지 않는다 |
| D-08 | 항목 활성 조건: 로컬 체크아웃이고, 셸이 PATH(RuntimeEnvironment)에서 `node`를 찾고, Herdr에 연결되어 포커스 pane이 있을 때. 아니면 항목은 비활성이고 툴팁이 이유 하나를 말한다("Node.js is not on PATH", "Not connected to Herdr", "Remote files open on their device"). 호스트 실행 실패(stderr 마지막 줄)는 notice로 보인다. | engineering/principles.md rule 4, 10; design/principles.md rule 9 |
| D-09 | 호스트 프로세스 소유: 셸이 띄운 `node` 프로세스는 열기 요청 하나에 하나이고 결과 JSON 또는 실패로 끝나며 30초 안에 끝나지 않으면 종료하고 notice를 낸다. 동시에 진행 중인 열기 요청은 파일당 하나다. | engineering/principles.md rule 14, 15 |
| D-10 | `design/hide.pen`에 `Screen / Editor / Document kinds`(text, markdown, image, pdf, binary, pdf 실패) 보드와 `Screen / Panel / Explorer` 보드의 파일 컨텍스트 메뉴 상태(활성, node 없음 비활성)를 그리고, `DESIGN.md`의 Explorer file management 문단과 File document toolbar 문단, `docs/BROWSER_PANES.md`(셸 진입점 추가)를 같은 PR에서 갱신한다. | AGENTS.md 디자인 캔버스 규칙, "Update the owning guide ... in the same change" |
| D-11 | 원칙 반영: engineering/principles.md와 design/principles.md(oh-my-principle fa5186d)를 전부 읽었다. rule 4·10은 D-08의 비활성 사유와 notice로, rule 11은 D-07의 pane 재사용으로, rule 14·15는 D-09로, design rule 3은 더블클릭 소스 유지·우클릭 열기로, design rule 5는 기존 NSMenu 항목 패턴과 HideIconButton 툴바 유지로, design rule 9는 B4·B9·B12~B15로 번역했다. engineering rule 7: PDFKit은 시스템 프레임워크라 새 패키지가 없다. | sasu principles list |
| D-12 | 배포: `agents/config.json`대로 run worktree에서 구현하고 PR로 전달하며 CI를 지켜본다. | agents/config.json delivery.mode=pr |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Explorer에서 `.pdf` 파일을 더블클릭하면(또는 확장자가 없어도 내용이 PDF면) 에디터 자리에 PDF가 연속 세로 스크롤로 창 폭에 맞춰 보이고, 텍스트를 드래그해 선택·복사할 수 있다. 문서 툴바에는 경로 브레드크럼, 비활성 Find, Reveal in Explorer, Reveal in Finder만 보인다. | D-01, D-02, D-03 |
| B2 | PDF 탭은 파일 탭 목록에 다른 파일과 같은 방식으로 들어가고, 탭 전환·닫기·재오픈이 텍스트 파일과 같다. "Unsaved"는 절대 보이지 않는다. | D-01, D-03 |
| B3 | 손상된 PDF를 열면 "PDF unavailable"과 디코드 실패 사유가 보이고 툴바는 그대로다. | D-03 |
| B4 | 이미지·텍스트·마크다운 파일의 열기 결과는 지금과 같다(같은 뷰, 같은 툴바). UTF-8이 아닌 파일은 "Preview only" 상태에 "This file type cannot be shown as text."가 보이고, 2 MB 초과·읽기 전용 사유 문구는 지금과 같다. | D-01, D-02 |
| B5 | 파일 행을 우클릭하면 메뉴가 New File, New Folder, 구분선, Open with Default App, Open in Browser Pane, 구분선, Reveal in Finder, Copy Path, Copy Relative Path, 구분선, Rename, 구분선, Delete 순으로 보인다. 폴더 행·빈 영역·원격 트리 메뉴는 지금과 같다. | D-04 |
| B6 | Open with Default App을 고르면 macOS가 그 파일의 기본 앱(PDF는 Preview, HTML은 기본 브라우저)으로 연다. macOS가 거부하면 기존 notice 자리에 "macOS could not open <path>: <reason>"이 보인다. | D-05 |
| B7 | HTML 파일을 더블클릭하면 지금처럼 소스 편집기가 열린다. | D-05 |
| B8 | Open in Browser Pane을 고르면 포커스 pane 오른쪽에 브라우저 pane이 열리고 그 파일이 `file://` URL로 렌더된다. 포커스는 이동하지 않는다. | D-06, D-07 |
| B9 | 같은 파일에 Open in Browser Pane을 다시 고르면 새 pane이 생기지 않고 기존 pane이 그대로 남는다(탐색·포커스 변화 없음). | D-07 |
| B10 | 브라우저 pane은 실행 중인 chromux 관리 프로필 중 가장 최근에 쓴 것으로 열린다. 어느 프로필이 쓰였는지는 pane 헤더의 기존 프로필 표시로 보인다. | D-06 |
| B11 | 실행 중인 chromux 프로필이 없으면 pane이 생기지 않고 notice "No running chromux profile. Launch one with chromux launch <name>."이 보인다. | D-06 |
| B12 | PATH에 `node`가 없으면 Open in Browser Pane 항목이 비활성이고 툴팁이 "Node.js is not on PATH"다. Herdr에 연결되지 않았으면 비활성이고 툴팁이 "Not connected to Herdr"다. | D-08 |
| B13 | 호스트가 실패하면(플러그인이 다른 설치본에 링크됨, 프로필 일시정지 등) notice에 호스트의 실패 문장이 그대로 보이고 다른 pane은 닫히지 않는다. 30초 안에 끝나지 않으면 "Browser pane did not open in time"이 보인다. | D-08, D-09 |
| B14 | 열기 진행 중에는 같은 파일의 Open in Browser Pane 항목이 비활성(툴팁 "Opening…")이고, 끝나면 다시 활성이다. | D-09 |
| B15 | 앱 번들의 `browser-pane.mjs profiles`는 각 프로필의 `running`과 `state_modified_at`을 포함한 JSON을 돌려주고, 기존 호출자(이름 목록만 읽던 에이전트)는 그대로 동작한다. | D-06 |
| B16 | `design/hide.pen`에 D-10의 두 보드가 있고 `node scripts/check-design-contract.mjs`가 통과하며, `DESIGN.md`와 `docs/BROWSER_PANES.md`가 새 메뉴 항목·PDF 뷰·셸 진입점을 설명한다. | D-10 |

## Technical structure

- `herdr-core/src/files.rs`, `model.rs`: 에디터 문서 스냅샷에 `document_kind` enum 필드 추가. C ABI 시그니처는 그대로이고 스냅샷 JSON만 넓어진다. 판정은 파일 앞부분 바이트와 확장자로 하며 `Mutex<Runtime>` 안에서 파일 전체를 다시 읽지 않는다(기존 읽기 경로 재사용).
- `macos/Sources/HerdrMacOS/EditorViewerOverlay.swift`: `editorContent`가 `document_kind` switch로 뷰를 고른다. PDF 뷰는 `PDFKit`(시스템 프레임워크) `PDFView`를 감싼 NSViewRepresentable 하나. 새 색·간격은 `HideTheme`에서만 온다.
- `WorkspaceOutlinePresentation.swift` / `WorkspaceOutlineView.swift`: 메뉴 항목 두 개와 활성 조건(`WorkspaceOutlineMenuPresentation`이 값으로 결정해 테스트가 물을 수 있게).
- 셸의 브라우저 pane 열기 어댑터(신규, `ExternalFileOpener`·`ExternalBrowser`와 같은 경계 종류): 번들 리소스 경로 해석, `node` 탐색, 프로필 선택, `Process` 실행과 30초 제한, 결과 JSON 파싱. 대상 pane은 스냅샷의 포커스 pane.
- `plugins/browser/browser-pane.mjs`: `profiles` 출력 확장(하위 호환), `browser-pane.test.mjs`에 케이스 추가.
- 새 크레이트·패키지 없음. 다른 서비스·스키마·인프라 변경 없음.

## Risks

- `node`가 로그인 PATH에만 있고 앱 PATH 해석이 놓치는 경우: 바운드는 B12의 비활성 사유. 검증은 설치본이 아닌 dev 빌드 하나의 PID를 특정해 PATH를 확인한다.
- 브라우저 pane 호스트가 다른 설치본(설치 앱 vs worktree dev 앱)에 링크되어 있으면 열기가 거부된다(B13). 검증 시 `herdr plugin list --plugin hide.browser`로 링크를 먼저 확인하고, 운영자의 Browser pane·서비스는 닫지 않는다.
- 이 머신에는 지금 실행 중인 chromux 프로필이 없다(`chromux ps`가 0). B8~B10 검증은 검증자가 `chromux launch <name>`으로 프로필 하나를 띄운 뒤 하고, 끝나면 자기가 띄운 것만 내린다. 운영자 프로필은 건드리지 않는다.
- PDFView와 SwiftUI 오버레이의 크기 협상: 첫 표시에서 0 크기가 되면 빈 화면이 된다. 바운드는 실제 AppKit 레이아웃 테스트(기존 native editor 테스트 패턴) 하나.
- 라이브 증명 경계: 로컬 파일만 열고 네트워크·자격 증명·운영자 pane 조작이 없다. 스크린샷은 `agents/runs/<slug>/`에만 둔다.
- 사용자가 미리 할 일: 없음.
- 열린 결정: 없음. 가정으로 표시한 D-02·D-03·D-04·D-05·D-07은 되돌릴 수 있는 구현 선택이며 최종 보고에 나열한다.
