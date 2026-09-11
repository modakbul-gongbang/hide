---
topic: "File Explorer 기본 파일 관리: 컨텍스트 메뉴, 새 파일·폴더, 이름 바꾸기, 경로 복사, 드래그 이동"
status: "ready"
human_approval: "approved"  # user 2026-09-11 verbatim: File Explorer 개선 하기: 링크 복사나 새 파일이나 그런것들은 다 되게 해야지 기본기능인데.. 우측 클릭했을 때 기본적으로 vscode처럼 그래도 관리할 수 있게 해주면 좋을 것 같아 (새폴더, 새파일정도는.. 그리고 드래그해서 옮길 수 있게 하거나도...) / 너가 적당히 승인해서 issue 로 넣는것까지 다 잘 해버려(queue로)
review_profile: "standard"
review_rationale: "사용자 작업 트리 안에서 파일을 만들고 이름을 바꾸고 옮기는 기능이라 잘못되면 파일이 사라질 수 있지만, 덮어쓰기와 루트 밖 경로를 거부하고 삭제는 별도 PRD로 분리해 파괴적 동작은 없다."
source_intake: "current conversation"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
screen_evidence: "screenshot"
created_at: "2026-09-11"
updated_at: "2026-09-11"
---

# PRD: File Explorer 기본 파일 관리: 컨텍스트 메뉴, 새 파일·폴더, 이름 바꾸기, 경로 복사, 드래그 이동

## Goal

Hide의 File Explorer에서 사람이 VS Code처럼 항목을 우클릭해 새 파일과 새 폴더를 만들고, 이름을 바꾸고, 경로를 복사하고, Finder에서 열고, 항목을 다른 폴더로 드래그해 옮길 수 있다.
지금 트리(`WorkspaceOutlineView.swift`)는 열기와 Enter만 있고 컨텍스트 메뉴, 생성, 이름 바꾸기, 경로 복사, 드래그가 하나도 없어 파일 하나 만들려고 터미널이나 Finder로 나가야 한다.
삭제는 확인 모달이 필요해 후속 PRD(explorer-delete-with-confirmation)가 이 메뉴와 코어 파일 이벤트 위에 얹는다.

## Non-goals

- 삭제: 후속 PRD가 같은 메뉴에 항목을 추가한다 (D-01).
- 파일 시스템 감시(FSEvents 등)로 트리를 자동 갱신하기: 이 PRD는 자기가 바꾼 부모 노드만 다시 읽는다. 재검토는 외부 변경이 트리에 안 보인다는 불만이 실제로 쌓일 때 (D-05).
- 복사·붙여넣기, 복제, 여러 항목 동시 선택과 이동: 트리는 단일 선택이다. 재검토는 이 PRD가 머지된 뒤 (D-06).
- Finder나 다른 앱에서 트리로 파일을 드롭해 가져오기, 트리에서 터미널 pane으로 드래그하기: 트리 안의 이동만 다룬다 (D-06).
- 원격 체크아웃 트리(`RightPanel.swift:79`)의 파일 변경: 원격은 읽기 전용이라 경로 복사 외의 항목을 보이지 않는다 (D-02).
- 새 항목의 템플릿이나 파일 확장자 추론: 이름은 사용자가 입력한 그대로다 (D-03).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 컨텍스트 메뉴 항목과 순서: New File, New Folder, (구분선) Reveal in Finder, Copy Path, Copy Relative Path, (구분선) Rename. Delete는 후속 PRD가 Rename 아래에 추가한다. 빈 영역 우클릭은 루트를 대상으로 New File, New Folder만 보인다. | 사용자: "vscode처럼 (새폴더, 새파일정도는)"; 기존 템플릿 `GitWorktreesView.swift:123-127`의 Reveal/Copy path |
| D-02 | 원격 체크아웃 트리는 Copy Path, Copy Relative Path만 보이고 나머지 항목은 없다. | `RightPanel.swift:79` 원격 분기는 읽기 전용 |
| D-03 | New File, New Folder, Rename은 트리 안 인라인 텍스트 필드로 이름을 받는다. Enter 확정, Esc 취소, 포커스 이탈은 취소. 빈 이름, `/` 포함, 같은 폴더의 기존 이름은 확정을 거부하고 필드 아래 한 줄로 이유를 보인다. 새 항목은 우클릭한 폴더(파일이면 그 부모) 안에 만든다. | VS Code 동작; 덮어쓰기는 어떤 경로로도 일어나지 않는다 |
| D-04 | 파일 시스템 변경은 Swift가 하지 않고 코어 dispatch로 간다. 새 kind `file_create`, `dir_create`, `path_rename`, `path_move`(`runtime.rs` payload/ValidatedEvent/decode 표, `files.rs` 구현). 코어는 루트 밖 경로와 기존 경로 덮어쓰기를 거부하고, 실제 fs 호출은 런타임 mutex 밖에서 한 뒤 결과를 한 이벤트로 낸다. 성공 시 선택 경로를 새 항목으로 옮긴다. | `AGENTS.md` Runtime Architecture: 셸은 UI 상태를 소유하지 않는다; `CoreBridge.swift:3030` 문서: 한 이벤트로 화면 효과가 함께 떨어져야 반쯤 움직인 화면이 없다; `docs/PERFORMANCE_TESTING.md` Runtime mutex 규칙 |
| D-05 | 변경 뒤 트리는 영향받은 부모 노드(이동은 출발·도착 둘)만 `loadChildren`으로 다시 읽고 펼침·선택 상태를 유지한다. 감시자는 두지 않는다. | `WorkspaceOutlineView.swift:594` 캐시 구조; 감시자는 범위 밖 |
| D-06 | 드래그는 트리 안 단일 항목 이동만이다(`NSOutlineView` 드래그 소스·드롭 대상). 폴더 위에 놓으면 그 안으로, 파일 위에 놓으면 그 파일의 부모로, 빈 영역은 루트로. 같은 부모·자기 자신·자기 하위 폴더로의 드롭은 드롭 표시가 뜨지 않는다. 도착지에 같은 이름이 있으면 이동을 거부하고 이유를 보인다. | 사용자: "드래그해서 옮길 수 있게"; 단일 선택 트리(`allowsMultipleSelection` 기본 false) |
| D-07 | Copy Path는 절대 경로, Copy Relative Path는 트리 루트(`model.focusedPath`) 기준 상대 경로를 `NSPasteboard.general`에 문자열로 넣는다. Reveal in Finder는 `ExternalFileOpener.reveal`을 쓴다. | `ShellModel.swift:1505` copyCheckoutPath 패턴; `ExternalFileOpener.swift:38` |
| D-08 | 실패(권한, 디스크, 코어 거부)는 해당 행 아래 한 줄 메시지로 보이고 트리는 변경 전 상태를 유지한다. 조용히 무시하거나 부분 적용하지 않는다. | engineering 4, 10: 실패는 호출자에게 보이는 결과로 |
| D-09 | 테스트: Rust 단위 테스트(`files.rs`: 생성·이름 바꾸기·이동 성공, 덮어쓰기 거부, 루트 밖 거부)와 Swift 프레젠테이션 테스트(노드 종류별 메뉴 항목 집합, 이름 검증, 상대 경로, 드롭 대상 계산). 러너는 `scripts/rust-test.sh`, `scripts/swift-test.sh`. | 기존 `RightPanelPresentationTests.swift`, `HerdrMacOSTests` 타깃 |
| D-10 | 증거: 실행 중인 dev 빌드에서 메뉴가 열린 상태, 인라인 새 파일 입력, 드래그 중 드롭 표시의 스크린샷을 찍어 task-factory `evidence/explorer-file-operations` 브랜치에 올린다. hide 리포에는 스크린샷을 커밋하지 않는다. 네이티브 검증은 Hide 인스턴스가 정확히 하나여야 하므로 운영자 앱이 열려 있으면 QA 창을 질문한다. | hide `AGENTS.md` 33-38행: 스크린샷은 커밋 금지; `docs/PERFORMANCE_TESTING.md` 129행 |
| D-11 | Delivery: hide의 sasu PR 모드(`agents/config.json`, base `main`, worktree, CI watch)로 Implementor가 PR을 열고 사람이 머지한다. | task-factory D-07 |
| D-12 | New File로 파일을 만들어 확정하면 그 파일이 곧바로 에디터 탭으로 열린다(`file_open`과 같은 효과). 이 열기는 `file_create` 성공 결과의 일부로 코어가 한 이벤트 안에서 처리해 트리 선택과 탭 열기가 같은 프레임에 떨어지고, 별도 클릭이나 두 번째 dispatch가 없다. New Folder, Rename, 드래그 이동은 탭을 열지 않는다. 파일 생성은 성공했지만 탭을 열 수 없는 경우는 B10과 같이 한 줄 이유로 보이고 파일은 남는다. | 사용자(PR #53 리뷰, 2026-09-11 verbatim): "근데 근데 파일 생성했으면 새 탭에 바로 열어줘야 되는거 아냐?"; VS Code의 New File 동작; D-04 한 이벤트 원칙 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 로컬 트리의 파일이나 폴더를 우클릭하면 New File, New Folder, Reveal in Finder, Copy Path, Copy Relative Path, Rename 순서의 메뉴가 뜬다. 빈 영역 우클릭은 New File, New Folder만 뜬다. | D-01 |
| B2 | 원격 체크아웃 트리에서 우클릭하면 Copy Path, Copy Relative Path만 뜬다. | D-02 |
| B3 | New File을 고르면 대상 폴더 안에 인라인 입력 행이 나타나고, 이름을 치고 Enter를 누르면 빈 파일이 생겨 트리에 보이며 선택된다. New Folder도 같고 빈 폴더가 생긴다. | D-03, D-04, D-05 |
| B4 | 인라인 입력에서 Esc를 누르거나 다른 곳을 클릭하면 아무것도 만들어지지 않고 입력 행이 사라진다. | D-03 |
| B5 | 빈 이름, `/`가 든 이름, 같은 폴더에 이미 있는 이름으로 확정하면 입력 행이 남은 채 아래에 이유가 한 줄 보이고 파일은 만들어지지 않는다. | D-03, D-08 |
| B6 | Rename을 고르면 그 행이 인라인 편집으로 바뀌고, 새 이름으로 Enter를 누르면 항목이 바뀐 이름으로 같은 자리에 보이며 선택이 유지된다. 기존 이름과 충돌하면 B5와 같이 거부된다. | D-03, D-04 |
| B7 | Copy Path 뒤 붙여넣으면 절대 경로가, Copy Relative Path 뒤에는 루트 기준 상대 경로가 나온다. Reveal in Finder는 Finder에서 그 항목을 선택해 보여준다. | D-07 |
| B8 | 항목을 드래그해 다른 폴더 위에 놓으면 그 폴더 안으로 옮겨져 출발 폴더에서 사라지고 도착 폴더에 보이며 선택된다. 파일 위에 놓으면 그 파일의 부모로, 빈 영역에 놓으면 루트로 간다. | D-06, D-04, D-05 |
| B9 | 같은 부모, 자기 자신, 자기 하위 폴더 위로는 드롭 표시가 뜨지 않고 놓아도 아무 일이 없다. 도착지에 같은 이름이 있으면 옮겨지지 않고 이유가 한 줄 보인다. | D-06, D-08 |
| B10 | 어떤 작업이든 실패하면 트리는 작업 전과 같고 이유가 한 줄 보인다. 반쯤 적용된 상태는 없다. | D-08, D-04 |
| B11 | 변경 뒤 펼쳐져 있던 다른 폴더는 그대로 펼쳐져 있다. | D-05 |
| B12 | `scripts/rust-test.sh`에 코어 파일 작업 테스트가, `scripts/swift-test.sh`에 메뉴·이름 검증·상대 경로·드롭 대상 테스트가 있고 통과한다. | D-09 |
| B13 | New File 인라인 입력에서 Enter로 파일을 만들면 그 파일이 즉시 에디터 탭으로 열려 편집할 수 있고, 트리에서도 선택돼 있다. New Folder를 만들거나 이름을 바꾸거나 드래그로 옮길 때는 탭이 열리지 않는다. | D-12, D-04 |

## Technical structure

- `herdr-core/src/runtime.rs`: payload 구조체(833-890행 부근), `ValidatedEvent` 변형(1134행 부근), kind 표(10995행 부근)에 `file_create`, `dir_create`, `path_rename`, `path_move` 추가. 핸들러는 `files.rs`에 두고 fs 호출은 mutex 밖에서 한다. 스냅샷 계약 변경은 없고 `uiState.selectedPath`만 갱신한다.
- `macos/Sources/HerdrMacOS/CoreBridge.swift`: `file_open`/`reveal_path`와 같은 형태의 dispatch 래퍼 4개.
- `macos/Sources/HerdrMacOS/WorkspaceOutlineView.swift`: `NSMenu` 컨텍스트 메뉴, 인라인 편집 셀, `NSOutlineView` 드래그 소스와 드롭 대상, 부모 노드 재로딩. 메뉴 구성·이름 검증·상대 경로·드롭 대상 계산은 뷰와 분리된 순수 프레젠테이션 타입으로 둬서 테스트한다.
- `RightPanel.swift`: 원격 분기에는 경로 복사만 연결.
- 새 설정, 영속 상태, 외부 서비스는 없다.

## Risks

- **Runtime mutex**: 파일 작업이 mutex 안에서 블로킹 I/O를 하면 hide PR 템플릿의 첫 질문에 걸린다. D-04대로 mutex 밖에서 처리하고 PR Risk surface에 답한다.
- **잘못된 이동**: 드롭 대상 계산 오류는 파일을 엉뚱한 곳에 옮긴다. 덮어쓰기 거부(D-06)와 Swift 테스트(D-09)로 막고, Finder Trash를 쓰지 않으므로 이 PRD에서 파일이 사라지는 경로는 없다.
- **네이티브 검증**: 운영자의 Hide가 떠 있으면 스크린샷을 못 찍는다. Implementor는 QA 창을 질문 코멘트로 요청한다(D-10).
- **후속 PRD 결합**: 삭제 PRD가 이 메뉴 구조와 코어 이벤트 패턴을 소비하므로, 메뉴 구성 타입과 dispatch kind 명명을 여기서 안정적으로 정한다.
- 사람이 미리 해야 할 일은 없다.
