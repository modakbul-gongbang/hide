---
topic: "Explorer, History, View 탭, 사이드바에서 OS 파일 관리자로 보기 (reveal_external)"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "네 메뉴의 항목과 데스크톱 호스트가 OS 파일 관리자에 경로를 넘기는 기존 IPC 경로를 바꾸는 일반 UI 변경이며, 실행 없이 선택만 하는 showItemInFolder를 그대로 쓰므로 데이터나 권한 경계는 바뀌지 않는다."
source_intake: "current conversation"
created_at: "2026-10-02"
updated_at: "2026-10-02"
---

# PRD: Explorer, History, View 탭, 사이드바에서 OS 파일 관리자로 보기

## Goal

hide 데스크톱 앱에서 파일을 다루는 운영자가 Explorer 행, History 행, View 탭, 사이드바 행 어디서든 우클릭 한 번으로 그 파일이나 폴더를 OS 파일 관리자(Finder, 파일 탐색기, Linux 파일 관리자)에서 부모 폴더 안에 선택된 채로 볼 수 있게 한다.
지금은 사이드바 행만 macOS 이름(`Reveal in Finder`)으로 이 동작을 갖고 있고, View 탭의 `Reveal in Explorer`는 앱 안 파일 트리에서 행을 고르는 다른 동작인데 이름이 Windows File Explorer와 겹친다.
하나의 액션 ID `reveal_external`과 OS를 따르는 라벨로 네 진입점을 맞추고, 앱 안 동작은 `Select in File Tree`로 이름을 바꾼다.

## Non-goals

- Explorer 메뉴에 Open with Default App, Copy Path, Copy Relative Path를 추가하지 않는다. 이슈는 reveal 항목만 요구하고 HANDOFF는 Open with Default App을 명시적으로 제외했다. 운영자는 경로가 필요하면 View 탭의 Copy path를 쓴다. 별도 이슈가 요구하면 다시 다룬다.
- 원격 디바이스의 파일을 그 디바이스의 파일 관리자로 여는 기능은 만들지 않는다. 항목은 이유와 함께 비활성화된다. 원격 helper가 OS 셸 동작을 노출하게 되면 다시 다룬다.
- 일반 브라우저 탭(데스크톱 앱이 아닌 셸)에서 OS 파일 관리자를 여는 경로는 만들지 않는다. 항목을 숨긴다.
- 파일 문서 툴바의 "두 reveal"(`docs/UI_BEHAVIOR.md` File document toolbar)은 코드에 없고 이 변경에서 만들지 않는다.
- core(`herdr-core`), hided, 스냅샷 와이어는 바꾸지 않는다. `.pen` 디자인 파일도 바꾸지 않는다.
- Windows, Linux용 데스크톱 패키지를 새로 만들지 않는다. 라벨 규칙만 모든 OS를 다룬다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Explorer 행, History 행, View 탭, 사이드바 프로젝트·체크아웃 행이 하나의 액션 ID `reveal_external`을 쓴다. 사이드바의 `reveal_finder`는 `reveal_external`로 이름을 바꾸고 옛 ID는 남기지 않는다. | 이슈 #324 "같은 액션 ID `reveal_external`"; HANDOFF 운영자 승인 "ㅇㅇㅇㅇ 너 제안대로"; 엔지니어링 원칙 1 |
| D-02 | 동작은 기존 `hideHost.revealPath` -> 데스크톱 호스트 `shell.showItemInFolder` 경로를 그대로 쓰며, 파일과 폴더 모두 부모 폴더에서 항목을 선택한 채로 OS 파일 관리자를 연다. 아무것도 실행하거나 열지 않는다. | 이슈 #324; 운영자 "OS에 상관없이 그냥 우클릭하면 Finder, Explorer 같은 게 외부로 열리게 하는 거" |
| D-03 | 라벨은 데스크톱 호스트의 OS를 따른다: macOS `Reveal in Finder`, Windows `Reveal in File Explorer`, Linux `Open Containing Folder`, 그 밖이나 알 수 없으면 `Show in File Manager`. OS는 데스크톱 호스트가 preload 브리지로 알려 준 값을 쓰고, 셸이 user agent로 추측하지 않는다. | 이슈 #324 라벨 목록; 가정: OS 파일 관리자는 호스트의 것이므로 호스트가 OS를 말한다 |
| D-04 | 원격 디바이스의 항목에서는 `reveal_external`을 보이되 비활성화하고 이유를 함께 보인다(`FINDER_HERE_ONLY` 패턴). 이유 문구는 OS 이름 없이 `Only for files and folders on this computer.`로 바꾼다. | 이슈 #324 "원격 디바이스에서는 이유와 함께 비활성화"; 가정: 문구는 OS 중립으로 바꾼다 |
| D-05 | 일반 브라우저 탭(호스트 브리지 없음)에서는 네 메뉴 모두 `reveal_external` 항목을 숨긴다. | 이슈 #324 "일반 브라우저 탭에서는 숨깁니다" |
| D-06 | View 탭 메뉴의 앱 안 동작 `Reveal in Explorer`(ID `reveal`)는 라벨 `Select in File Tree`, ID `select_in_tree`로 바꾸고 동작은 그대로 둔다. | 이슈 #324 "앱 내부 동작은 `Select in File Tree`로"; 가정: ID도 라벨과 맞춘다(원칙 1) |
| D-07 | 메뉴 배치: Explorer 파일 행은 기존 Open 항목들(Open to the side, HTML이면 Open in Browser), 구분선, reveal, 구분선, Rename, 구분선, Move to Trash. 폴더 행은 New File, New Folder, 구분선, reveal, 구분선, Rename, 구분선, Move to Trash. 행 아래 빈 영역(루트)은 기존대로 두 생성만. History 행은 Open to the side, 구분선, reveal. View 탭은 Copy path, Select in File Tree, reveal, 구분선, Close view. 사이드바는 기존 `Reveal in Finder` 자리를 그대로 쓴다. | `docs/UI_BEHAVIOR.md` Explorer file management의 VS Code 순서; 가정: 현재 메뉴 구성 위에 reveal만 끼운다 |
| D-08 | 대상이 없는 것으로 이미 알려진 항목은 비활성화하고 이유를 보인다: History의 deleted 상태 행(`The file was deleted.`), View 탭에서 파일이 unavailable이거나 아직 읽을 수 없는 표시(기존 `revealBlocked` 이유). 브라우저 페이지 표시에는 파일이 아니므로 reveal 항목이 없다. | 가정: 디자인 원칙 "모든 상태를 설계한다"; 엔지니어링 원칙 4 |
| D-09 | 데스크톱 호스트는 경로를 넘기기 전에 절대 경로인지(기존 `revealablePath`)와 존재하는지 확인하고, 거부와 성공을 경로 없이 진단 로그에 남긴다(`reveal.refused` with reason `sender`/`path`/`missing`, 성공 이벤트). 화면에는 알림을 띄우지 않는다. | 디자인 원칙 13; 엔지니어링 원칙 4, 10; 가정 |
| D-10 | Explorer 메뉴에는 reveal만 추가하고 Copy Path, Copy Relative Path, Open with Default App은 넣지 않는다. `docs/UI_BEHAVIOR.md`의 Explorer 메뉴 문단은 실제 메뉴를 설명하도록 고쳐 쓴다. | HANDOFF "Add only what the issue asks (the reveal item)... leave Open with Default App out"; 가정: 복사 항목은 후속 |
| D-11 | `docs/UI_BEHAVIOR.md`의 Explorer, View 탭 메뉴, 사이드바 메뉴 문구를 같은 변경에서 고친다. | 이슈 #324 "문구를 함께 고칩니다"; `CLAUDE.md` "Update the owning guide" |
| D-12 | 전달은 `gen-prd/reveal-external` 브랜치의 GitHub PR로 하며, CI를 지켜보고 병합하지 않는다. PR 제목과 본문은 한국어로 저장소 템플릿 순서를 따른다. 로컬 전달이나 병합은 기각한다. | HANDOFF "It is NOT approval to merge"; `agents/config.json` delivery.mode `pr` |
| D-13 | 선언된 원칙 저장소 `~/projects/oh-my-principle`는 이 머신에서 `ROOT.md`가 없어 `sasu principles list`가 실패했다. 대신 HANDOFF가 적은 디자인 원칙(기존 패턴 따르기, 상태를 시각으로, 모든 상태 설계, 원칙 13)과 세션 훅이 주입한 엔지니어링 원칙 1-12를 적용한다. | `sasu principles list --json` 실패 출력; HANDOFF Design context |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | macOS 데스크톱 앱에서 로컬 체크아웃의 Explorer 파일 행을 우클릭하면 메뉴가 Open to the side(와 HTML이면 Open in Browser), 구분선, `Reveal in Finder`, 구분선, Rename, 구분선, Move to Trash 순서로 보인다. | D-01, D-03, D-07 |
| B2 | Explorer 폴더 행을 우클릭하면 New File, New Folder, 구분선, `Reveal in Finder`, 구분선, Rename, 구분선, Move to Trash 순서로 보인다. 행 아래 빈 영역의 메뉴는 New File, New Folder뿐이다. | D-07 |
| B3 | Explorer 파일이나 폴더 행에서 reveal을 고르면 OS 파일 관리자가 그 항목의 부모 폴더를 열고 항목을 선택한다. 파일은 실행되거나 열리지 않는다. | D-02 |
| B4 | History 행을 우클릭하면 Open to the side, 구분선, OS 라벨의 reveal 순서로 보이고, reveal을 고르면 그 파일이 부모 폴더에서 선택된 채로 OS 파일 관리자가 열린다. 기존 Open to the side는 그대로 옆에 연다. | D-01, D-02, D-07 |
| B5 | History의 deleted 상태 행에서는 reveal 항목이 비활성화되고 `The file was deleted.` 이유가 보인다. | D-08 |
| B6 | 파일이나 Diff View 탭을 우클릭하면 Copy path, `Select in File Tree`, OS 라벨의 reveal, 구분선, Close view가 이 순서로 끝에 보이고, `Reveal in Explorer`라는 항목은 어디에도 없다. | D-01, D-06, D-07 |
| B7 | View 탭의 `Select in File Tree`는 이전 `Reveal in Explorer`와 같게 도구 열을 Explorer로 보이고 그 행을 펼쳐 선택하며 파일을 열지 않는다. | D-06 |
| B8 | View 탭의 reveal은 그 파일을 부모 폴더에서 선택한 채로 OS 파일 관리자를 연다. 파일이 unavailable이거나 아직 읽을 수 없으면 비활성화되고 기존 이유가 보인다. 브라우저 페이지 탭에는 `Select in File Tree`도 reveal도 없다. | D-02, D-08 |
| B9 | 사이드바 프로젝트 행과 체크아웃 행의 기존 `Reveal in Finder` 자리는 같은 `reveal_external` 항목이며, 고르면 그 폴더가 부모 폴더에서 선택된 채로 OS 파일 관리자가 열린다. | D-01, D-02 |
| B10 | 라벨은 호스트 OS를 따른다: macOS `Reveal in Finder`, Windows `Reveal in File Explorer`, Linux `Open Containing Folder`, 그 밖 `Show in File Manager`. 네 메뉴가 같은 호스트에서 같은 라벨을 보인다. | D-03 |
| B11 | 원격 디바이스의 체크아웃에서 네 메뉴 모두 reveal 항목이 보이되 비활성화되고 `Only for files and folders on this computer.` 이유가 보인다. 고를 수 없으므로 아무것도 열리지 않는다. | D-04 |
| B12 | 일반 브라우저 탭으로 연 셸에서는 네 메뉴 어디에도 reveal 항목이 없고, 나머지 항목과 순서는 그대로다(View 탭은 Copy path, Select in File Tree, Close view). | D-05 |
| B13 | 메뉴 사이에 reveal이 사라진 파일이나 셸이 보낸 값이 절대 경로가 아니면 OS 파일 관리자는 열리지 않고, 데스크톱 로그에 경로 없이 거부 이유가 남는다. 성공도 경로 없이 로그에 남는다. 화면에는 알림이 뜨지 않는다. | D-09 |
| B14 | `docs/UI_BEHAVIOR.md`의 Explorer 메뉴, View 탭 메뉴, 사이드바 메뉴 문단이 위 메뉴 구성과 라벨 규칙, 원격·브라우저 규칙을 설명하고, Explorer 문단은 더 이상 메뉴에 없는 Open with Default App, Copy Path, Copy Relative Path를 약속하지 않는다. | D-10, D-11 |

## Technical structure

core, hided, 와이어는 바뀌지 않는다.
웹 셸에서 네 메뉴는 하나의 `reveal_external` 항목 규칙(라벨, 원격 비활성, 브라우저 숨김)을 공유하고 기존 `actions`의 reveal 액션 하나로 `hostBridge().revealPath`를 부른다.
데스크톱 preload 브리지는 호스트 OS를 한 값으로 노출하고, 메인 프로세스의 기존 REVEAL 채널이 존재 확인을 더해 `shell.showItemInFolder`를 부른다.
새 IPC 채널이나 의존성은 없다.

## Risks

- Linux의 `shell.showItemInFolder`는 파일 관리자에 따라 선택 없이 폴더만 열 수 있다. 라벨 `Open Containing Folder`가 그 차이를 흡수하며, 이 머신에서는 macOS만 실제로 관찰할 수 있다.
- 이 Mac mini는 화면이 잠겨 있던 적이 있어 Finder 창이 실제로 뜨는 네이티브 관찰이 불가능할 수 있다. 그때는 메뉴 구성 테스트와 호스트 IPC 단위 테스트로 대신하고 관찰하지 못한 것을 PR에 명시한다.
- `/Applications/hide.app`(운영자의 앱)은 바꾸거나 조작하지 않는다. 네이티브 확인은 후보 빌드와 격리된 Herdr에서만 한다.
- 사용자에게 미리 필요한 작업은 없다.
