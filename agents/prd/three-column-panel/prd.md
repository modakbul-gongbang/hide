---
topic: "사이드 패널을 Agent Views, File Views, Tools 세 컬럼으로 나누기"
status: "ready"
human_approval: "approved"
review_profile: "high-risk"
review_rationale: "출시된 #170 결정(파일을 열어도 터미널이 리사이즈되지 않음)을 뒤집어 PTY 리사이즈 시점을 바꾸고, 운영자의 Workspace별 저장 상태 파일(workspace-views.json)의 패널 키를 한 번 읽어 새 키로 바꾸므로 배치 손실과 터미널 회귀가 핵심 위험이다."
source_intake: "current conversation"
created_at: "2026-10-02"
updated_at: "2026-10-04"
---

# PRD: 사이드 패널을 Agent Views, File Views, Tools 세 컬럼으로 나누기

## Goal

Explorer를 켜 둔 채 에이전트 옆에서 파일이나 페이지를 보는 운영자에게, Workspace 본문을 왼쪽부터 Agent Views | File Views | Tools 세 개의 도킹된 컬럼으로 보여 준다.
지금은 View와 Tools가 Agent 영역 위에 떠 있는 카드 하나를 공유해서 메뉴, 툴팁, 네이티브 페이지가 패널과 겹치고, 패널 안의 Tools 토글·Expand·Pin과 툴바의 개수 뱃지 아이콘 하나가 전체를 여닫아 읽기 어렵다(이슈 #321).
확인할 목표 문장: "Explorer나 History에서 연 파일은 File Views 컬럼의 탭으로 열리고, File Views와 Tools는 각각 툴바 아이콘과 ⌘⇧B, ⌘E로 따로 켜고 끄며, 어떤 컬럼도 다른 컬럼 위에 뜨지 않는다."
이슈 #322(File View 탭 스트립)와 #323(페이지 위 오버레이)이 이 컬럼 구조 위에서 이어지므로 지금 정한다.

## Non-goals

- File View 탭 스트립을 Agent 탭 스트립과 같은 모양·축소 규칙으로 맞추는 일은 하지 않는다(#322). File Views 컬럼은 지금의 View 영역 탭 스트립을 그대로 쓰며, #322가 이 컬럼 위에서 다시 연다.
- 메뉴, 대화상자, 팝오버, 툴팁이 네이티브 페이지와 겹칠 때의 still 고정과 툴팁 배치는 바꾸지 않는다(#323). 이 PRD는 패널이 페이지 위에 뜨는 원인만 없애고, `OVERLAY_SELECTOR`는 건드리지 않는다. #323이 그 범위를 소유한다.
- 레이아웃 후보 B(Agent | Tools | File Views)는 만들지 않는다. 운영자가 A를 골랐다(D-01). 컬럼 순서를 바꾸는 설정도 만들지 않는다. 운영자가 순서 변경을 요청하면 다시 본다.
- Expand(File Views가 본문 전체를 덮는 상태)는 없앤다(D-06). 넓게 읽고 싶으면 Tools를 숨기거나 경계선을 끈다. 운영자가 Expand를 계속 쓴다고 하면 다시 본다.
- Agent Views 컬럼 안의 Agent 영역 split, View 영역 split·drag·preview·Open to the side 규칙, Herdr의 tab/pane/zoom 권한은 바꾸지 않는다. 바뀌는 것은 세 컬럼이 본문을 나누는 방식뿐이다.
- `contracts/hided-ws.schema.json`, Herdr API 계약, Herdr pin은 바꾸지 않는다. `workspace_view` 이벤트와 스냅샷은 지금도 이 schema에 정의되어 있지 않고 core가 유일한 정의다.
- `.pen` 파일을 이 PRD 작업에서 손으로 고치지 않는다. 화면 보드는 운영자 승인 절차(D-13)를 따른다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Workspace 본문은 왼쪽부터 Agent Views, File Views, Tools 세 컬럼이다(후보 A). 후보 B(Agent, Tools, File Views)는 기각한다. Tools가 오른쪽 끝에서 나타나고 사라져도 File Views 위치는 그대로다. | 이슈 #321 "레이아웃 후보 A ... B를 와이어프레임으로 비교했고 A를 선호했습니다"; HANDOFF: 운영자 "음 괜찮은 것 같은데?"; 와이어프레임 `wireframe-layout-options.html` |
| D-02 | 세 컬럼은 항상 도킹되며 떠 있는 패널, Pin, 패널이 본문을 덮었다는 보고(`panel_covers`, `covered`)를 없앤다. #170 트레이드오프는 도킹으로 해결한다: File Views나 Tools가 보이거나 숨거나, 경계선 drag가 놓이거나, 창 폭이 컬럼 수 경계를 넘을 때 Agent 터미널이 한 번 리사이즈된다. drag 도중, File Views 안에서 파일을 열고 닫고 탭을 바꿀 때, 에이전트를 고를 때는 리사이즈되지 않는다. 운영자는 PR 리뷰에서 이 비용을 명시적으로 확인한다. | 이슈 #321 "항상 도킹되므로 Pin은 없어집니다", "이 트레이드오프는 명시적으로 결정해야 합니다"; 가정: A를 고르고 Pin 제거를 정한 것이 리사이즈 비용 수용이며, 리사이즈 시점 경계는 작성자 가정 |
| D-03 | Explorer, History, ⌘P, 터미널 링크, `hide file open`, Open in Browser, Open server로 연 파일·diff·페이지는 모두 File Views 컬럼의 View 영역 탭으로 열린다. Agent 탭과 Agent 영역은 건드리지 않는다. 배치 규칙(preview, 이미 열린 표시로 이동, Open to the side)은 그대로다. | 이슈 #321 "Explorer나 History에서 연 파일은 File Views의 탭으로 열리고, Agent 탭은 건드리지 않습니다"; core 사실: 파일 열기는 이미 Agent 영역을 대상으로 하지 않음 |
| D-04 | 툴바는 Workspace 본문 전체 폭에 걸치고, 오른쪽 끝에 아이콘만 Open server, File Views, Tools 순서로 둔다. 마우스를 올리거나 키보드 focus가 가면 툴팁에 이름과 단축키 keycap이 보인다(Open server는 단축키 없음). File Views 아이콘은 File Views가 보이지 않는 동안 열린 view 개수 뱃지를 단다. 각 컬럼에는 제목 줄이 없고 첫 줄이 그 컬럼의 탭 줄이다. | 이슈 #321 툴바 항목; HANDOFF "the toolbar looks fine"; 와이어프레임 툴바 폭; HANDOFF 설계 원칙 "earn every container" |
| D-05 | ⌘⇧B는 File Views만, ⌘E는 Tools만 토글한다. 툴바 아이콘과 단축키는 같은 규칙이다: 보이지 않는 컬럼은 보이게 하고, 보이는 컬럼은 끈다. 디스패치는 "토글"이 아니라 결과 값(켜짐/꺼짐)을 실어 같은 이벤트가 두 번 와도 같은 상태로 수렴한다. Settings에서 두 명령의 키를 바꿔 둔 운영자의 바인딩은 새 명령으로 이어진다. 데스크톱 앱 View 메뉴의 두 항목 이름도 바뀐다. | HANDOFF "`⌘⇧B` now toggles File Views only, `⌘E` toggles Tools"; 가정: 결과 값 디스패치(engineering 원칙 11), 바인딩 승계 |
| D-06 | Expand를 없앤다. 도킹에서 File Views가 본문 전체를 차지하려면 Agent Views를 숨겨 터미널을 두 번 리사이즈하거나 덮는 모드를 다시 들여야 하는데, 이 변경이 없애려는 것이 그 덮는 모드다. 넓게 보려면 Tools를 숨기거나 경계선을 끈다. 저장된 `expanded`는 File Views 켜짐으로 읽는다. `hide` CLI와 Workspace 제어는 Expand를 쓰지 않는다. | HANDOFF "Expand stays only if still used; the PRD should say which"; 가정: 제거(engineering 원칙 2), 운영자가 거부할 수 있음 |
| D-07 | 좁은 창은 오버레이 없이 컬럼을 숨긴다. 기준은 창이 아니라 Workspace 본문 폭이며 컬럼 최소 폭은 Agent Views 480px, File Views 360px, Tools 260px로 유지한다. 경계선 폭까지 포함한 기준은 1116px와 848px다. 본문이 1116px 이상이면 켜진 컬럼이 모두 보이고, 848px 이상 1116px 미만이면 옆 컬럼은 하나만 보이며 Tools를 먼저 숨기고, 848px 미만이면 한 컬럼만 보이고 기본은 Agent Views다. 창 폭이 1440px이고 열린 사이드바 때문에 본문이 1100px인 경우에도 두 컬럼 단계로 처리하며 최소 폭을 줄이지 않는다. 숨겨진 컬럼을 운영자가 명시적으로 부르면(아이콘, 단축키, 파일 열기, reveal) 그 컬럼이 다른 옆 컬럼(848px 미만이면 Agent Views)의 자리를 대신하고, 848px 미만에서 에이전트를 고르면 Agent Views로 돌아온다. 이 좁은 창 배치는 표시 전용이며 저장하지 않는다. 창이 다시 넓어지면 저장된 켜짐/꺼짐과 폭이 돌아온다. | 이슈 #321의 본문 폭 기준과 명시 호출 시 자리 교체; 2026-10-04 운영자 승인: 480/360/260px 유지, 경계선 포함 1116/848px, 1440px 창의 사이드바 열린 본문 1100px는 두 컬럼 단계 유지 |
| D-08 | 컬럼 표시 여부는 Workspace 표현 상태이므로 core의 `workspace_view` 상태가 Workspace마다 소유한다: File Views 켜짐, Tools 켜짐, 보이는 도구(Explorer 또는 History), File Views 폭, Tools 폭. `panel`(closed/open/expanded), `pinned`, `covered`, `views_over_share`는 없앤다. 한 사용자 동작은 한 core 이벤트다. | HANDOFF "Panel visibility is Workspace presentation, so it belongs in the core's `workspace_view` state"; ARCHITECTURE.md Runtime Architecture "A user action is one event" |
| D-09 | 업그레이드 시 저장된 Workspace 항목은 한 번 새 키로 읽는다: `panel`이 `open` 또는 `expanded`면 File Views 켜짐, `closed`면 꺼짐; `tools`와 `tool`은 그대로; `pinned`, `views_over_share`, `tools_share`는 버리고 두 폭은 기본값으로 시작한다. View 영역 트리, Agent 영역, View 북마크는 그대로다. 다음 저장은 새 키만 쓰고 schema는 키 변경이므로 2로 둔다. 읽을 수 없는 파일은 지금처럼 옆에 보존되고 진단이 남으며 기본값으로 시작한다. | HANDOFF "What happens to a stored per-Workspace panel state (closed/open/expanded/pinned) on upgrade"; ARCHITECTURE.md:678 키 추가 이행 관례(운영자 저장 배치를 지키기 위해 engineering 원칙 1보다 우선); 가정: 폭은 기본값 |
| D-10 | File Views는 비어 있지 않다. 마지막 view가 닫히면 core가 같은 전이에서 File Views를 끈다. view가 하나도 없을 때 ⌘⇧B나 File Views 아이콘은 File Views에 기존 New tab 페이지(Open with File ⌘P, Diff)를 연다. Tools를 끄거나 켜도 File Views는 바뀌지 않고, Tools만 켜진 상태도 정상이다. | UI_BEHAVIOR.md "There is no empty panel body"를 컬럼에 적용; 가정: 빈 File Views 진입점으로 기존 New tab 페이지 재사용 |
| D-11 | 네이티브 페이지는 File Views 컬럼 안의 자기 slot에 도킹된 채 그려지고, 셸의 어떤 컬럼이나 패널도 그 위에 뜨지 않는다. File Views를 숨기면 페이지는 닫히지 않고 숨으며 다시 보이면 같은 주소로 돌아온다. 경계선 drag 동안은 기존 shell drag 규칙대로 모든 페이지가 still로 바뀐다. 메뉴·대화상자·팝오버·툴팁과 페이지의 겹침은 #323이 소유하며 이 PRD와 병합 순서에 묶이지 않는다. #323의 "사이드 패널을 오버레이 대상에 추가" 항목은 이 변경으로 필요 없어진다. | HANDOFF "How native browser pages sit in File Views, given issue #323"; 이슈 #323 본문; BROWSER_DISPLAYS.md Overlays |
| D-12 | File Views와 Tools는 각자의 폭을 Workspace마다 저장하고 Agent Views가 나머지를 갖는다. 컬럼 사이 경계선은 끌면 안내선이 따라오고 놓을 때 한 번 반영되며, focus된 경계선은 화살표 키로 한 단계씩 움직인다. 어느 컬럼도 D-07의 최소 폭보다 좁아지지 않는다. | 가정: Agent가 나머지를 갖는 단순 규칙, 기존 패널 grip 동작 재사용 |
| D-13 | 운영자는 2026-10-04 야간 지시의 "② 위임"으로 기존 승인 방향 안의 최종 디자인 판단을 위임했다. 위임받은 검토자가 실제 Pen 보드(`Screen / Workspace`의 세 컬럼, 툴바 아이콘 묶음과 툴팁, 경계선, 좁은 창 두 단계)와 다크·라이트 네이티브 후보 캡처를 비교해 최종 디자인을 승인한다. 기존 `Component / Side panel*` master는 컬럼 master로 대체하며, B35는 보드 없이 면제하지 않는다. | 운영자 "① 유지, ② 위임, ③ 간체·OS 자동, ④ 유지, ⑤ main까지"; DESIGN_WORKFLOW.md의 실제 보드·네이티브 비교 절차; 기록된 위임은 보드 부재를 승인했다는 뜻이 아님 |
| D-14 | `docs/UI_BEHAVIOR.md`의 Web Workspace 도입부, The side panel(세 컬럼 절로 교체), Opening, preview, and Open to the side의 "panel is closed" 문장, 재시작 복원 문단과 이행 문장, Browser displays의 shell drag 문장, Narrow windows, Library masters를 고친다. `docs/ARCHITECTURE.md` Workspaces in the web shell의 패널 상태·#170·`panel_covers`·`panelFrame`·좁은 창 문단과 D-08 area intent 문장, `docs/BROWSER_DISPLAYS.md`의 패널 가장자리 drag와 툴바 globe 문장도 같은 변경에서 고친다. | HANDOFF "Which `docs/UI_BEHAVIOR.md` sections change"; AGENTS.md "Update the owning guide ... in the same change" |
| D-15 | 원칙 intake: `sasu principles list`는 `~/projects/oh-my-principle`에 ROOT.md가 없어 실패했다. 그 저장소 커밋 8b0d709의 `engineering.md` 전문을 읽었다. design 원칙 문서는 이 machine에 없어 HANDOFF가 적은 목록(기존 패턴 따르기, 상태를 문장이 아닌 시각으로, 모든 컨테이너는 존재 이유가 있어야 함, 모든 상태 설계, 원칙 13)을 썼다. engineering 1(Pin·Expand·`panel_covers`·Tools 오버레이 삭제, 단 저장 배치는 D-09), 2(D-06), 4·10(읽을 수 없는 상태 파일의 보존과 진단), 11(D-05)을 반영했다. 12는 구현의 테스트 선택이라 행으로 옮기지 않았다. | 가정: 원칙 저장소 부분 사용 |
| D-16 | 2026-10-04 야간 지시와 "⑤ main까지"는 이전 PRD ONLY 실행 범위를 대체한다. 승인된 PRD를 구현하고 D-13의 위임된 최종 디자인 검토를 완료한 뒤 PR 모드(commit, push, PR, CI 확인)로 제출한다. 현재 구현자는 merge하지 않으며, 조정자만 독립 리뷰와 현재 최종 SHA의 필수 CI 통과를 확인해 보호된 main에 merge commit으로 병합한다. | 운영자 "⑤ main까지"; agents/config.json delivery.mode=pr; 조정자가 병합과 이슈 종료를 소유하며 직접 main push와 이력 재작성은 제외 |
| D-17 | 에이전트를 고르는 동작(사이드바, 팔레트, 탭 순환, `remote_control`)은 넓은 창에서 컬럼 표시를 바꾸지 않는다. 떠 있는 패널을 닫던 규칙은 덮는 패널이 없으므로 없어진다. | D-02의 귀결; ARCHITECTURE.md D-08 area intent 규칙 대체 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | Workspace를 열면 툴바 아래 본문이 왼쪽부터 Agent Views, File Views, Tools 컬럼으로 나뉘어 보이고, 컬럼끼리 겹치지 않으며 어떤 컬럼도 다른 컬럼 위에 그림자나 카드로 떠 있지 않다. 꺼진 컬럼은 자리를 차지하지 않는다. | D-01, D-02 |
| B2 | 처음 보는 Workspace는 Agent Views만 보이고 File Views와 Tools는 꺼져 있으며, 도구는 Explorer로 정해져 있다. | D-08, D-10 |
| B3 | Tools 컬럼의 Explorer에서 파일을 단일클릭하면 File Views 컬럼의 활성 View 영역 preview 탭으로 열리고, History 행을 누르면 같은 방식으로 diff가 열린다. Agent Views의 탭 줄, 활성 탭, 터미널 내용은 그대로다. | D-03 |
| B4 | File Views가 꺼진 상태에서 Explorer, History, ⌘P, 터미널 링크, Open in Browser, Open server로 무언가를 열면 File Views가 켜지면서 그 탭이 보인다. `hide file open`이나 `hide browser open`을 `--reveal` 없이 실행하면 탭은 생기지만 File Views의 켜짐/꺼짐은 바뀌지 않고 File Views 아이콘의 뱃지 수만 늘어난다. | D-03, D-04 |
| B5 | Explorer의 reveal(파일 위치 보기)은 Tools를 켜고 Explorer로 바꾼 뒤 그 파일까지 폴더를 펼친다. File Views 켜짐/꺼짐은 바뀌지 않는다. | D-03, D-08 |
| B6 | 툴바는 본문 전체 폭에 걸치고 왼쪽에 경로(`Home / Project / Workspace`), 오른쪽 끝에 Open server, File Views, Tools 아이콘이 이 순서로 있다. 툴바에는 그 밖의 패널 토글, Pin, Expand가 없다. | D-04, D-02, D-06 |
| B7 | 세 아이콘에 마우스를 올리거나 키보드로 focus하면 툴팁에 이름과 단축키 keycap이 보인다: `Open server`(keycap 없음), `File Views ⌘⇧B`, `Tools ⌘E`. Settings에서 키를 바꿨으면 바뀐 키가 보인다. 아이콘에는 글자 라벨이 없다. | D-04, D-05 |
| B8 | File Views와 Tools 아이콘은 그 컬럼이 화면에 보이는 동안 눌린 모양이고, 보이지 않으면 눌리지 않은 모양이다. 각 아이콘은 하나의 접근성 이름과 pressed 상태를 갖는다. | D-04, D-05 |
| B9 | File Views가 보이지 않는 동안 view가 열려 있으면 File Views 아이콘에 열린 view 개수 뱃지가 붙고, 같은 수가 접근성 설명으로도 주어진다. File Views가 보이면 뱃지는 사라진다. | D-04 |
| B10 | Open server는 지금처럼 알려진 listener가 하나면 바로, 여럿이면 키보드로 고르는 팝오버로 열고, 열린 페이지는 File Views 컬럼의 탭이 된다. 비었음·불러오는 중·끊김·실패·오래됨 상태와 원격 Workspace의 로컬 탐색 없음 안내도 같은 팝오버에 그대로 있다. | D-03, D-04 |
| B11 | ⌘⇧B나 File Views 아이콘은 File Views만 보이게 하거나 끄고, Tools와 Agent Views의 탭·도구 선택은 그대로다. 다시 켜면 View 영역 트리, 탭, preview, 활성 view가 끄기 전 그대로 돌아온다. | D-05, D-08 |
| B12 | ⌘E나 Tools 아이콘은 Tools만 보이게 하거나 끄고, 고른 도구(Explorer 또는 History)를 유지한다. File Views의 켜짐/꺼짐과 view는 바뀌지 않는다. | D-05, D-10 |
| B13 | 같은 토글이 빠르게 두 번 도착하거나 같은 결과 값이 다시 전달되어도 컬럼은 운영자가 마지막으로 요청한 상태에 머물고, 중간 상태가 저장되거나 깜빡이지 않는다. | D-05, D-08 |
| B14 | 데스크톱 앱의 View 메뉴에 `Toggle File Views`(⌘⇧B)와 `Toggle Tools`(⌘E)가 있고, 누르면 단축키와 같은 결과다. Settings, Shortcuts에서 두 명령의 키를 바꿔 둔 운영자는 업그레이드 후에도 바꾼 키가 각각 File Views와 Tools를 토글한다. | D-05 |
| B15 | 툴바를 우클릭하거나 메뉴 키를 누르면 `Show/Hide File Views`, `Show/Hide Tools`, `Copy Workspace path`, `Open Project Overview`가 나오고, 패널 세 상태나 Pin/Unpin 항목은 없다. | D-02, D-05, D-06 |
| B16 | view가 하나도 없을 때 ⌘⇧B나 File Views 아이콘을 누르면 File Views가 기존 New tab 페이지(Open with File ⌘P, 변경이 있으면 Diff) 하나를 담고 열린다. 손대지 않은 그 탭을 닫으면 File Views가 꺼진다. | D-10 |
| B17 | File Views의 마지막 view를 닫으면 File Views 컬럼이 같은 순간에 사라지고 Agent Views가 그 폭을 받으며, Tools가 켜져 있으면 Tools는 그대로 남는다. 빈 File Views 본문이나 빈 상태 문구는 보이지 않는다. | D-10, D-12 |
| B18 | File Views를 켜거나 끄거나, Tools를 켜거나 끄거나, 경계선 drag를 놓으면 Agent 터미널이 새 폭에 맞춰 한 번 리사이즈되고 입력 유실, 재attach, 화면 공백 없이 계속 쓸 수 있다. | D-02, D-12 |
| B19 | File Views 안에서 파일을 열고, 탭을 바꾸고, view를 닫고(마지막 view 제외), View 영역을 split하거나 drag하는 동안 Agent 터미널의 크기는 바뀌지 않는다. 경계선을 끄는 동안에도 안내선만 움직이고 터미널은 놓을 때까지 그대로다. | D-02, D-12 |
| B20 | Agent Views와 File Views 사이, 그리고 Tools 왼쪽의 경계선에 마우스를 올리거나 focus하면 경계선 모양이 보이고, 끌면 안내선이 포인터를 따라오며, 놓으면 한 번 반영된다. focus된 경계선은 왼쪽·오른쪽 화살표로 한 단계씩 움직이고 각 경계선은 separator 역할과 현재 값을 보조기술에 알린다. | D-12 |
| B21 | 경계선을 아무리 끌어도 Agent Views는 480px, File Views는 360px, Tools는 260px보다 좁아지지 않는다. 바꾼 File Views와 Tools 폭은 Workspace마다 저장되어 재시작 후에도 같다. | D-07, D-12 |
| B22 | 넓은 창에서 사이드바, 팔레트, 탭 순환, 다른 기기의 에이전트로 에이전트나 탭을 고르면 그 에이전트가 Agent Views에 focus되고 File Views와 Tools는 그대로 보인다. | D-17 |
| B23 | Agent Views의 pane과 탭은 항상 클릭과 입력을 받고, File Views나 Tools에 키보드가 있을 때 ⌘F, ⌘T, ⌥W는 키보드가 있는 곳(View 영역, Tools, pane)에서 동작한다. | D-02 |
| B24 | 키보드가 있는 컬럼을 숨기면 키보드는 File Views가 보이면 그 활성 View 영역으로, 아니면 focus된 에이전트 pane으로 돌아가고, 보이지 않는 컬럼으로 Tab이 들어가지 않는다. | D-05 |
| B25 | Workspace 본문이 경계선 포함 1116px 이상이면 켜진 컬럼이 모두 보인다. 848px 이상 1116px 미만이면 File Views와 Tools가 둘 다 켜져 있어도 File Views만 보이고 Tools는 숨는다. 848px 미만이면 Agent Views만 보인다. 창 폭 1440px에서 사이드바가 열려 본문이 1100px이면 File Views만 보이고 Tools는 숨으며 480/360/260px 최소 폭은 그대로다. 어느 경우에도 컬럼이 다른 컬럼 위에 오버레이로 뜨지 않는다. 사이드바를 ⌘B로 숨겨 본문이 넓어지면 그 폭으로 다시 판단한다. | D-07 |
| B26 | 848px 이상 1116px 미만에서 ⌘E, Tools 아이콘, reveal로 Tools를 부르면 Tools가 File Views 자리에 보이고 Tools 아이콘이 눌리며 File Views 아이콘은 눌리지 않는다. 그 상태에서 Explorer로 파일을 열거나 ⌘⇧B를 누르면 File Views가 다시 그 자리에 온다. | D-07 |
| B27 | 848px 미만에서 File Views나 Tools를 부르거나 파일을 열면 그 컬럼 하나가 본문을 차지하고, 사이드바·팔레트·탭 순환으로 에이전트를 고르거나 같은 아이콘을 다시 누르면 Agent Views로 돌아온다. View 영역이 모두 들어가지 않으면 지금처럼 활성 영역만 영역 전환기와 함께 보인다. | D-07 |
| B28 | 좁은 창에서 숨긴 컬럼은 저장 상태를 바꾸지 않는다. 창이나 본문을 다시 넓히면 Workspace에 저장된 File Views와 Tools의 켜짐/꺼짐, 폭, 도구, View 영역 크기가 그대로 돌아오고, 다른 Workspace의 컬럼 상태는 좁은 창 동안에도 바뀌지 않는다. | D-07, D-08 |
| B29 | 재시작하면 마지막 Workspace의 File Views 켜짐/꺼짐, Tools 켜짐/꺼짐과 도구, 두 폭, View 영역 트리와 탭이 남겨 둔 대로 돌아온다. 페이지는 마지막 주소로, 미저장 텍스트는 초안에서 돌아온다. | D-08, D-11 |
| B30 | 이전 빌드에서 패널을 연(open) 상태나 펼친(expanded) 상태로 둔 Workspace는 업그레이드 후 File Views가 켜진 채, 닫힌(closed) 상태였던 Workspace는 File Views가 꺼진 채 열린다. Pin 여부와 관계없이 도킹되고, Tools와 도구 선택, 열린 view, 탭, 영역 배치는 그대로이며, 두 컬럼 폭은 기본값이다. | D-09 |
| B31 | 상태 파일을 읽을 수 없거나 알 수 없는 값이 들어 있으면 원본 파일이 옆에 보존되고 진단 로그에 사유가 남으며, 앱은 기본 상태로 시작해 운영자가 Workspace를 골라 이어 간다. 화면에 경고 배너는 뜨지 않는다. | D-09, D-15 |
| B32 | 네이티브 페이지는 File Views 컬럼 안의 자기 자리에 정확히 그려지고 Tools나 다른 셸 표면이 그 위를 덮지 않는다. 경계선을 끌거나 컬럼을 켜고 끌 때 페이지는 다시 로드되지 않고 새 자리에 맞춰 옮겨진다. | D-11, D-12 |
| B33 | File Views를 끄면 페이지는 닫히지 않고 화면에서 사라지며, 다시 켜면 같은 주소와 상태로 돌아온다. 경계선을 끄는 동안 페이지는 still로 보이고 놓으면 다시 살아난다. | D-11 |
| B34 | 원격 기기의 Workspace에서도 같은 세 컬럼, 단축키, 좁은 창, 저장 규칙이 적용되며, 한 Workspace의 컬럼 상태가 다른 Workspace나 다른 기기의 같은 경로 Workspace에 번지지 않는다. | D-08 |
| B35 | 세 컬럼, 툴바 아이콘 묶음과 툴팁, 뱃지, 경계선(쉼, hover, focus, drag), 좁은 창 두 단계, Tools만 켜진 상태가 운영자가 승인한 Pen 보드와 맞고, 다크와 라이트, 한글과 긴 경로 제목에서 잘림 없이 읽힌다. | D-13, D-04 |

## Technical structure

기존 core → hided/WebSocket → React 경계를 유지하고 Herdr 계약은 바꾸지 않는다.
core의 Workspace별 `WorkspaceView`는 패널 필드(`panel`, `pinned`, `views_over_share`, 런타임 `covered`)를 File Views 켜짐, Tools 켜짐, 도구, File Views 폭, Tools 폭으로 바꾸고, `workspace_view` 이벤트 payload와 스냅샷이 같은 필드를 싣는다.
`panel_covers` 이벤트와 `covered` 필드, 에이전트 선택이 패널을 닫던 area intent는 지운다; 파일·diff·페이지 열기와 reveal이 컬럼을 켜는 area intent는 새 필드로 옮긴다.
마지막 view가 닫힐 때 File Views를 끄는 결정은 core가 같은 전이에서 한다.
`workspace-views.json`은 schema 2를 유지하며 옛 패널 키를 한 번 새 키로 읽고 다음 저장부터 새 키만 쓴다; 기존 v1과 S6 `mode` 읽기는 새 키를 만들도록 바꾼다.
`ui_state.right_panel_visible`/`right_panel_section` 투영은 Tools 켜짐과 도구에서 나온다(저장하지 않음).
웹은 절대 배치된 `SidePanel` 오버레이와 `panelFrame`, 좁은 창 Tools 오버레이(`toolsPlacement`)를 지우고, 본문을 도킹된 세 컬럼과 두 경계선으로 그린다; 좁은 창 단계와 자리 교체는 웹의 표시 전용 상태다.
컬럼 최소 폭은 `design/tokens.json`의 토큰으로 두고 `scripts/gen-tokens.mjs`로 내린다.
단축키 레지스트리의 두 명령은 기존 id를 유지해 저장된 바인딩을 잇고, 제목과 동작만 바꾼다; 데스크톱 메뉴는 레지스트리에서 그대로 만들어진다.

## Risks

- #170 결정의 역전: 도킹이면 File Views/Tools 토글과 경계선 반영 때마다 모든 Agent 터미널이 한 번 리사이즈되어, 긴 출력 중인 에이전트의 줄바꿈이 바뀐다. 리사이즈를 동작당 한 번으로 묶고 drag 중에는 막는 것(B18, B19)으로 한정하며, `docs/PERFORMANCE_TESTING.md` 절차로 토글 시 PTY resize 횟수와 입력 에코를 측정한다. 운영자는 PR 리뷰에서 이 비용 수용을 확인한다(D-02).
- 2026-10-04 운영자가 승인한 폭 기준은 480/360/260px 최소 폭과 경계선 포함 1116/848px 두 단계 및 자리 교체(D-07)다. 1440px 창에서 사이드바가 열린 본문 1100px도 두 컬럼 단계로 처리하며 최소 폭을 줄이지 않는다. Expand 제거(D-06), 업그레이드 시 폭 기본값(D-09), 빈 File Views 진입점(D-10)은 기존 결정대로 유지한다.
- 구현과 최종 제출 조건: 기록된 운영자 승인과 디자인 판단 위임 아래 구현을 진행한다. 최종 제출 전 실제 Pen 보드와 대체 컬럼 master, 다크·라이트 네이티브 비교와 위임된 최종 디자인 승인(D-13)을 완료한다. B35가 미실행이면 완료로 보고하지 않는다.
- 저장 상태 이행 실패는 배치 손실로 보인다. 원본 보존·진단·기본값 시작의 기존 경로(B31)로 한정하고, View 영역 트리와 북마크는 옮기지 않고 그대로 읽는다.
- #323이 먼저 병합되면 그 PR이 사이드 패널을 오버레이 대상에 추가했을 수 있다. 이 변경에서 패널이 사라지므로 그 항목은 함께 지운다. 툴팁이 Tools가 꺼진 동안 File Views의 페이지 위로 내려오면 페이지 아래에 그려질 수 있으며, 이는 #323의 툴팁 배치 범위다.
- 네이티브 검증: 운영자 앱을 건드리지 않고 격리된 후보 창을 PID로 지정해 캡처해야 하며, 화면이 잠긴 machine에서는 미실행으로 기록한다.
- 이 PRD는 2026-10-04 야간 작업 지시로 승인됐다. Pen 보드 검증은 D-13을 따르며 자격 증명이나 외부 비용은 없다.
