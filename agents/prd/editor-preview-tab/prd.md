---
topic: "Editor preview tab (VS Code 방식)"
status: "ready"
human_approval: "approved"  # user 2026-09-19 verbatim: 승인, 그대로 진행해
review_profile: "standard"
review_rationale: "A user-visible change to how the editor tab strip opens and replaces file and diff tabs, with no persisted data, credentials, or external effect beyond the pull request."
source_intake: "current conversation"
created_at: "2026-09-19"
updated_at: "2026-09-19"
---

# PRD: Editor preview tab (VS Code 방식)

## Goal

Hide에서 Explorer의 파일을 한 번 클릭할 때마다 영구 탭이 하나씩 쌓여, 파일을 훑어보기만 해도 탭 스트립이 금방 가득 찬다(`open_file_tab`은 파일마다 탭을 push한다).
VS Code의 preview tab 모델을 그대로 들여온다: 한 번 클릭은 체크아웃당 하나뿐인 교체형 미리보기 탭(기울임꼴 제목)을 열고, 더블클릭·편집·명시적 Keep Open이 그 탭을 일반 탭으로 승격하며, 수정된 탭은 절대 교체되지 않는다.
사용자는 파일을 아무리 훑어봐도 탭이 늘지 않고, 계속 볼 파일만 의도적으로 남긴다.

## Non-goals

- preview를 끄는 설정 토글(VS Code `workbench.editor.enablePreview`): v1은 항상 켜져 있다. 끄고 싶은 사용자가 나타나면 재검토 (D-08).
- JetBrains식 탭 개수 상한·LRU 자동 닫기, Sublime식 "탭 없는 transient 뷰": VS Code 모델을 골랐으므로 채택하지 않는다 (D-01).
- Recent Panels 전환기와 Explorer 행에 기울임꼴을 확장하는 것: preview 표시는 탭 스트립에서만 한다. 스트립 밖에서 preview 여부를 구별해야 하는 요구가 생기면 재검토 (D-06).
- 편집기 탭의 앱 재시작 복원: 편집기 탭은 지금처럼 ephemeral이며, preview 상태를 저장할 곳이 없다 (D-07).
- 원격 체크아웃: 원격 컨텍스트에는 파일 탭이 없으므로(`rebuild_tab_strips`) 대상이 아니다.
- `design/principles.md` 5(기존 패턴을 따른다)는 이 PRD가 새 UI 패턴을 만들지 않고 스트립·툴팁·아이콘 버튼의 기존 컴포넌트만 쓰는 것으로 만족하며, 관찰 가능한 행으로 따로 두지 않는다 (D-14).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | VS Code의 preview tab 모델을 채택한다: 한 번 클릭 = 교체형 preview 탭, 더블클릭·편집·Keep Open = 승격. JetBrains(탭 상한), Sublime(transient 뷰), Xcode(현재 탭 교체) 모델은 기각. | 사용자 "VS Code 방식으로 PRD 잡아줘" (2026-09-19); 네 모델 비교는 같은 대화의 앞 턴 |
| D-02 | preview 탭은 체크아웃당 정확히 하나이고, 파일 탭과 diff 탭이 같은 슬롯을 공유한다. Changes 패널의 한 번 클릭도 preview diff 탭을 연다. | 사용자 "one per checkout"; 가정: 스트립이 파일·diff를 한 줄에 섞어 그리므로 diff만 영구로 쌓이면 문제가 그대로 남는다. VS Code SCM 뷰의 diff도 preview로 열린다 |
| D-03 | 승격 트리거는 네 가지: Explorer 행 더블클릭, 탭 제목 더블클릭, 첫 draft 변경(편집), 메뉴 명령 Keep Open. 탭을 드래그해 순서를 바꾸는 것도 승격한다. | 사용자 "double-click, edit, or explicit keep promotes it"; 드래그 승격은 VS Code와 같은 동작을 유지하려는 가정 |
| D-04 | 교체는 제자리에서 일어난다: 새 preview 탭은 기존 preview 탭의 스트립 슬롯을 물려받고, 교체된 탭의 문서·Markdown 모드·wrap 상태는 버려진다. 교체된 preview 탭은 Recent Closed에 기록하지 않는다. | VS Code와 같은 동작; 가정: 훑어본 파일마다 Recent Closed에 남기면 목록이 사용자가 닫은 탭이 아닌 것으로 채워진다 |
| D-05 | 수정된(dirty) 탭은 어떤 경우에도 교체되지 않는다. 첫 draft 변경이 승격이므로 정상 경로에서 dirty preview는 생기지 않지만, 코어는 교체 전에 dirty를 검사하고 dirty면 승격 후 새 preview를 옆에 연다. | 사용자 "dirty tabs never replaced"; `engineering/principles.md` 4(잘못된 상태를 조용히 넘기지 않는다) |
| D-06 | preview 탭은 탭 스트립에서 제목을 기울임꼴로 그린다. 기울임꼴 폰트 변형은 `HideTheme`에 토큰으로 추가하고 인라인 값은 쓰지 않는다. 툴팁과 접근성 라벨에는 "Preview"를 덧붙인다. | 사용자 "italic label"; `AGENTS.md` Design Reference(새 값은 HideTheme에 먼저) |
| D-07 | preview 여부는 코어가 소유한다: `EditorTabSnapshot`과 스트립 항목에 `preview` 플래그를 두고, 셸은 `file_open`/`changes_select`에 preview 요청 여부를, 승격은 새 이벤트 하나로 보낸다. 교체·승격 판단은 코어 안에서만 한다. 영속 UI 상태에는 넣지 않는다. | `docs/ARCHITECTURE.md` "코어가 UI 상태를 소유하고 셸은 권한이 없다"; `DESIGN.md` "편집기 탭은 ephemeral" |
| D-08 | preview를 끄는 설정은 두지 않는다. | `engineering/principles.md` 2(현재 요구를 충족하는 가장 단순한 구현), 7(기존 것에 기댄다) |
| D-09 | Explorer 한 번 클릭 외의 진입점(Cmd+P 파일 검색, Recent Closed 재열기, Markdown 상대 링크)은 영구 탭으로 연다. | VS Code 기본값(`enablePreviewFromQuickOpen`, `enablePreviewFromCodeNavigation` 모두 false); 가정: 검색해 찾아간 파일은 의도적으로 연 파일이다 |
| D-10 | 이미 영구 탭으로 열린 파일을 한 번 클릭하면 그 탭을 포커스만 하고 preview를 만들지 않는다. 현재 preview인 파일을 한 번 클릭하면 포커스만 한다. | VS Code와 같은 동작; 한 파일이 두 탭으로 열리지 않는다는 기존 `prepare_file_tab` 규칙 유지 |
| D-11 | 코어 규칙(교체·승격·dirty 거부·슬롯 상속·Recent Closed 제외)은 Rust 런타임 테스트로, 스트립의 기울임꼴·툴팁은 Swift 프레젠테이션 테스트로 고정한다. | `agents/config.json`의 `verify-cargo.sh`/`verify-swift.sh`; `engineering/principles.md` 12 |
| D-12 | Delivery: hide의 sasu PR 모드(`agents/config.json`: base `main`, worktree, CI watch)로 PR을 열고 사람이 머지한다. | `agents/config.json` `delivery.mode: pr` |
| D-13 | 설계 캔버스: `design/hide.pen`에 `Screen / Workbench / Preview tab` 보드를 preview·승격·dirty 세 상태로 그리고, 기울임꼴 토큰을 `scripts/pen-token-map.json`에 매핑한 뒤 구현을 시작한다. | `AGENTS.md` The Design Canvas("PRD가 바꾸는 화면을 Screen 보드로 먼저 그린다") |
| D-14 | Principles intake: `~/projects/oh-my-principle` `fa5186d`의 `engineering/principles.md`와 `design/principles.md`를 전부 읽었다. engineering 2·4·7·12, design 3·7·9는 위 결정과 행으로 번역했고, design 5는 non-goal로 뒀다. | `sasu principles list` 결과 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Explorer에서 파일을 한 번 클릭하면 그 파일이 스트립 끝에 새 탭으로 열리고 활성화되며, 탭 제목이 기울임꼴이다. | D-01, D-02, D-06 |
| B2 | 같은 체크아웃에서 preview 탭이 열린 채 다른 파일을 한 번 클릭하면, 새 파일이 그 preview 탭의 슬롯을 제자리에서 물려받고 이전 파일은 스트립에서 사라진다. 탭 개수는 늘지 않는다. | D-02, D-04 |
| B3 | 체크아웃마다 preview 탭이 하나씩 따로 있다. 다른 체크아웃의 파일을 한 번 클릭해도 이 체크아웃의 preview 탭은 그대로다. | D-02 |
| B4 | Explorer에서 파일을 더블클릭하면 영구 탭(정체 제목)으로 열린다. 그 파일이 현재 preview 탭이면 같은 슬롯에서 제목만 정체로 바뀐다. | D-03 |
| B5 | preview 탭에서 첫 글자를 편집하면 그 프레임에서 제목이 정체가 되고 탭이 영구가 된다. 이후 다른 파일을 한 번 클릭하면 새 preview 탭이 옆에 열린다. | D-03, D-05 |
| B6 | 탭 제목을 더블클릭하거나 메뉴의 Keep Open을 실행하면 preview 탭이 제자리에서 영구 탭이 된다. 영구 탭에서는 두 동작 모두 아무것도 바꾸지 않는다. | D-03 |
| B7 | preview 탭을 드래그해 순서를 바꾸면 영구 탭이 된다. | D-03 |
| B8 | 어떤 경로로든 preview 탭이 dirty인 채로 다른 파일이 한 번 클릭되면, dirty 탭은 그대로 남아 영구가 되고 새 파일은 새 preview 탭으로 옆에 열린다. 편집 중인 내용은 사라지지 않는다. | D-05 |
| B9 | 이미 영구 탭으로 열린 파일을 한 번 클릭하면 그 탭이 포커스되고 preview 탭은 만들어지지도, 교체되지도 않는다. | D-10 |
| B10 | 현재 preview 탭인 파일을 한 번 더 클릭하면 포커스만 되고 탭은 그대로다. | D-10 |
| B11 | Cmd+P 파일 검색, Recent Closed 재열기, Markdown 미리보기의 상대 파일 링크로 연 파일은 영구 탭이다. | D-09 |
| B12 | Changes 패널에서 변경 파일을 한 번 클릭하면 preview diff 탭이 열리고, 같은 체크아웃의 preview 파일 탭이 있으면 그 슬롯을 물려받는다. 파일 탭과 diff 탭은 하나의 preview 슬롯을 나눠 쓴다. | D-02, D-04 |
| B13 | preview 탭을 닫기 버튼이나 Cmd+W로 닫으면 지금과 같이 Recent Closed에 기록되고, 이전 활성 탭으로 돌아간다. | D-04 |
| B14 | 다른 파일에 의해 교체된 preview 탭은 Recent Closed에 나타나지 않는다. | D-04 |
| B15 | 한 번 클릭한 파일을 읽을 수 없으면 지금과 같은 `file.open_failed` 오류가 보이고, 기존 preview 탭은 교체되지 않고 그대로 남는다. | D-04 |
| B16 | preview 탭의 툴팁과 접근성 라벨은 파일명에 "Preview"를 덧붙여 읽히고, 승격되면 그 접미사가 사라진다. | D-06 |
| B17 | preview 탭의 활성·비활성·hover 색과 닫기 버튼·단축키 힌트는 다른 탭과 같고, 기울임꼴만 다르다. | D-06 |
| B18 | preview 탭은 Recent Panels 전환기에 다른 파일 탭과 같은 모양으로 나열된다. | D-06 |
| B19 | 앱을 재시작하면 지금처럼 편집기 탭은 하나도 남지 않는다. | D-07 |
| B20 | preview 탭이 교체될 때 Explorer의 선택 강조는 새 파일로 옮겨 간다. | D-02 |

## Technical structure

코어가 preview 상태를 소유한다: `EditorTabSnapshot`과 `StripTabSnapshot`에 `preview` 플래그가 추가되고, `file_open`과 `changes_select` 이벤트가 preview 요청 여부를 실어 오며, 승격은 새 편집기 이벤트 하나로 들어온다.
교체·승격·dirty 거부는 모두 `Mutex<Runtime>` 안의 편집기 런타임에서 결정되고, 셸은 스냅샷을 그리기만 한다.
셸은 Explorer 아웃라인의 더블클릭 액션, 탭 제목 더블클릭, Keep Open 메뉴 명령(단축키 레지스트리에 등록)을 추가하고, `HideTheme`에 기울임꼴 폰트 토큰을 더한다.
Herdr 와이어 계약, 영속 UI 상태, 파일 저장 경로, Markdown 미리보기는 바뀌지 않는다.

## Risks

- AppKit `NSOutlineView`는 더블클릭의 첫 클릭에서 `action`을 먼저 보내므로 더블클릭은 항상 "preview로 열림 → 승격"의 두 프레임으로 보인다. VS Code도 같은 순서로 동작하므로 허용하되, 두 번째 프레임이 제목만 바꾸고 문서를 다시 읽지 않는지 확인한다.
- 편집 승격(B5)은 `file_draft`가 코어에 닿는 순간 일어난다. 셸이 draft를 debounce하면 그 사이 한 번 클릭이 들어올 수 있고, 그 경우 B8이 dirty 탭을 지킨다. 구현은 B8을 Rust 테스트로 고정한다.
- 기울임꼴은 `HideTheme` 토큰과 `hideFont` 경로로만 들어가야 `check-design-contract.mjs`를 통과한다. pen 캔버스는 JetBrains Mono/Inter 대체 글꼴이라 기울임꼴 형태는 정확하지 않으며, 크기와 간격만 검토 대상이다.
- 설계 보드(D-13)는 구현 시작 전에 `design/hide.pen`에 그려야 하고 `node scripts/check-pen.mjs`를 통과해야 한다. 아직 그리지 않았다.
- 사용자에게 필요한 사전 작업은 없다.
