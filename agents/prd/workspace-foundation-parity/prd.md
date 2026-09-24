---
topic: "workspace-foundation-parity"
status: "draft"
human_approval: "pending"
review_profile: "high-risk"
review_rationale: "기기별 파일 쓰기·삭제, Git worktree 수명, 초안 마이그레이션과 원격 실행 권한을 하나의 경계로 통합한다."
source_intake: "current conversation"
created_at: "2026-09-24"
updated_at: "2026-09-24"
---

# PRD: Workspace Foundation Parity (S5.5)

## Goal

사용자는 어느 기기를 선택하든 같은 Workspace 화면과 명령으로 프로젝트를 찾고, 파일을 읽고 편집하고, Git 변경을 확인하며 작업 공간을 관리한다.
S6의 화면 재구성 전에 대상 기기·프로젝트·checkout·문서·작업의 식별과 권한을 일관되게 만들고, 연결 전환이나 실패가 다른 기기의 데이터에 영향을 주지 않도록 기존 구조를 정리한다.

## Non-goals

- S6의 탐색·All Agents·세 모드·도구 재배치, S7의 split/preview·복수 문서 표시·앱 재시작 복원, S8의 Project Memory·보관 Sessions, S9의 통합 UX 검수, S10의 Swift 삭제는 부모 roadmap의 후속 범위다.
- S7을 위한 문서와 표시 위치의 식별 분리는 이번 범위지만 새 split UI는 제공하지 않는다.
- Git stage/discard/commit, 자동 push, Git URL을 통한 Memory·설정 동기화는 추가하지 않는다.
- 운영 기기의 앱 교체, 서비스·SSH 신뢰 설정 변경, 무인 설치, 기존 작업 종료는 구현 검증에 포함하지 않는다.
- 원격 agent hook 설치와 원격 AI 설정 복제는 기존 금지를 유지한다.
  사용자는 소유 호스트의 설정과 대상 기기의 상태를 구별해서 보며, 원격 설정 쓰기는 별도 권한 계약에서 다시 다룬다.
- Swift 병행 경로는 S10까지 유지한다.
  공통 서비스로 이관한 중복 로직만 같은 변경에서 제거하며, engineering 원칙 1의 필수 전환 경로 예외를 적용한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 로컬·원격에 같은 기능 계약과 상호작용을 적용하고 S6보다 먼저 완료한다. | 사용자: “난 local, remote 다 동일한 인터페이스에서 동작하는게 가장 중요한것같은데”, “그거 먼저 진행하자 S6전에”; 부모 roadmap 커밋 `2a28d82`. |
| D-02 | local은 브라우저가 아니라 연결한 hided 호스트를 뜻하며, 모든 대상 식별은 기기까지 포함한다. | 현행 `docs/ARCHITECTURE.md`, `web/src/remote.ts`; 가정: 호스트명·경로 표시와 불변 기기 식별자를 분리한다. |
| D-03 | 원격 catalog도 Project → Checkout → Workspace/Tab/Pane 관계를 제공하고 기존 프로젝트 식별 규칙을 확장한다. | `herdr-core/src/workspace.rs`, `hide-project/src/lib.rs`; 현재 원격 raw workspace 목록은 로컬 catalog와 동등하지 않다. |
| D-04 | 기기·checkout·문서마다 하나의 편집 버퍼와 초안이 있고 탭은 그 버퍼의 표시 위치다. | `web/src/buffers.ts`, `herdr-core/src/runtime/editor.rs`; 가정: 기기 없는 기존 초안은 출처 확인 전 별도 복구 항목으로 보존한다. |
| D-05 | 파일 기능은 하나의 안전 계약을 로컬·원격 adapter가 구현하고 Git 변경 계산은 별도 소유자가 담당한다. | `hided/src/boundary.rs`, `herdr-core/src/files.rs`, `changes.rs`, `remote_files.rs`; 사용자: “관련 구조 정리”, “안전한 파일/Git/Workspace/설정 경계를 구체화”. |
| D-06 | 경로·권한·revision 검증 실패 시 쓰기를 거절하고 초안을 보존한다. | 현행 열린 root handle·regular-file·크기·revision 검사; 원격 read 후 write와 로컬 검사 후 truncate 모두 원자적 CAS라는 근거는 없다. |
| D-07 | 파일 생성·이동·이름 변경·휴지통 및 worktree 관리의 기존 안전 조건을 원격에도 적용한다. | S3·S5 계약과 `files.rs`, `runtime/projects.rs`, `worktree_cleanup.rs`; 신규 원격 실행 권한은 D-20의 차단 결정이며 아직 승인되지 않았다. |
| D-08 | 명령은 대상·충돌 범위·generation·operation ID를 가진 하나의 의도이며 timeout은 결과 불명으로 다룬다. | `runtime/operations.rs`, `runtime/agents.rs`; ACK는 결과 확정이 아니고 유한 중복 캐시는 영속 exactly-once 보장이 아니다. |
| D-09 | Herdr가 pane·PTY·분할·zoom·cwd·agent 생명주기를 소유하고 core가 표시 탭·포커스·패널을 소유한다. | `docs/ARCHITECTURE.md`; [CLI](https://herdr.dev/docs/cli-reference/)·[Socket API](https://herdr.dev/docs/socket-api/), 실제 pin/schema가 API 판단 기준이다. |
| D-10 | 등록 해제, 기기 연결 해제, pane 닫기, worktree 삭제를 서로 다른 명령으로 유지한다. | `runtime/projects.rs`; 등록 해제는 확인한 pane을 닫고 등록만 제거하며 디렉터리·worktree·Herdr session을 삭제하지 않는다. |
| D-11 | Settings는 값의 소유 호스트와 적용 범위를 드러내며 선택한 원격 기기가 daemon 소유 설정의 저장 위치를 바꾸지 않는다. | `web/src/SettingsSheet.tsx`, `hide-ai/src/settings.rs`, `docs/agent-hooks.md`, S5 D06; 원격 hook 쓰기와 standalone hided hook 설치는 기존 지원 범위 밖이다. |
| D-12 | capability는 기능·버전·실제 권한·연결 상태를 구분하며 일시적 미준비와 영구 미지원의 이유 및 다음 행동을 제공한다. | `herdr-core/src/remote.rs`; 가정: capability 미확인 상태는 비활성으로 시작하고 조회 실패를 빈 성공으로 바꾸지 않는다. |
| D-13 | 인증된 WS 파일 전송과 명시적 첨부 의도를 유지하며 브라우저 위치로 파일 소유 호스트를 추론하지 않는다. | S3/S4 계약, `hided/src/boundary.rs`; 외부 열기의 `untrusted_client` 경계와 원본 pane에 고정된 첨부를 유지한다. |
| D-14 | root 변경·기기 전환·재접속에도 오래된 응답은 새 화면이나 다른 파일에 적용하지 않는다. | 기존 generation fence와 operation 구조; 가정: 기기·checkout·문서·요청 generation을 응답까지 전달한다. |
| D-15 | 검색·파일 전송·초안·watch·대기 작업의 기존 상한을 유지하고 새 원격 작업도 취소와 상한을 가진다. | `boundary.rs`, `buffers.ts`, `remote_files.rs`; 가정: 기기당 실행 4개·대기 32개, 동일 문서 쓰기는 실행 1개·최신 대기 1개, 한 번의 탐색은 50,000개 항목 이내이며 기기 전체 초안 저장은 512MiB 또는 브라우저 quota 중 작은 값까지다. |
| D-16 | 기존 UI 패턴·디자인 토큰·접근성 동작을 유지하고 실패는 사용자가 행동할 위치에만 표시한다. | `DESIGN.md`, `design/tokens.json`; design 원칙 5·9·12·13, engineering 원칙 4·10. |
| D-17 | 대상과 읽기 → 문서와 저장 → Git·Workspace 명령 → 설정·재접속·중복 제거 순서로 수직 기능을 완성한다. | 인계의 제안 A–D를 가역적 작성자 가정으로 채택하며 PR 개수나 확정 작업 목록으로 해석하지 않는다. |
| D-18 | 완료는 양방향 기기 선택과 실패·복구를 실제 격리 환경에서 확인한 결과로 판단한다. | 인계와 `docs/PERFORMANCE_TESTING.md`; 코드 읽기나 연결 성공만으로 parity 완료를 주장하지 않는다. |
| D-19 | PRD 작성 후 Mac mini의 Opus 5.5 pane에 implement를 위임하되 문서 승인은 pending으로 남기고 차단 결정 해소 후 시작한다. | 최신 사용자: “ㅇㅇ 정리 다 되 implement로 mac mini에 opus5.5 로 pane 띄워서 작업하게 해줘!!”; 최초 요청의 push/PR/merge·앱 교체·운영 설정/서비스 변경 금지는 유지하며 `agents/config.json`의 PR 기본값보다 우선한다. |
| D-20 | **차단·미결정:** 안전한 원격 파일·worktree 기능에 필요한 대상 호스트 실행 및 설치 권한은 사용자가 결정한다. | 제안: 기기별 명시적 opt-in으로 버전 확인한 최소 helper를 설치·갱신하고 SSH 수명 안에서만 실행하며, UI에서 확인한 휴지통 이동·비강제 worktree 삭제만 허용한다; 무인 설치·상주 서비스·영구 삭제 fallback·원격 hook/AI 설정 쓰기는 금지한다; 제안은 승인이 아니다. |
| D-21 | 기술 상세와 UI 상태 배치는 거부 가능한 작성자 가정이며 동등 기능 전체를 미지원 처리해 완료하지 않는다. | 사용자: “가역적 기술 세부는 작성자 가정으로 정하고 진짜 미결정만 한 번에 보고”; 예외는 이유·사용자 결과·재검토 조건을 명시한다. |
| D-22 | 현행 계약과 원칙을 기준으로 관련 구조만 정리한다. | `docs/README.md`, `ARCHITECTURE.md`, `PERFORMANCE_TESTING.md`, `DESIGN.md`; 원칙 저장소 `654485f96b7764c759662d2c3e9e386ebc221cf6`의 engineering·design 전문과 env/test/process practices를 읽었다; engineering 1–15는 경계·실패·수명·자원·중복 제거에, design 1–13은 기존 흐름과 상태에 적용하며 새 레이아웃 선택은 S6로 유보한다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 어느 호스트의 hided를 열었든 기기 선택 후 동일한 Project·Checkout·Workspace·파일·Git·설정 진입점을 사용하며 선택 대상의 이름과 연결 상태를 확인한다. | D-01, D-02, D-12 |
| B2 | 서로 다른 기기의 같은 절대 경로와 같은 Git URL은 별개 프로젝트·초안·명령 대상으로 유지되고 이름 변경이나 재접속으로 새 기기가 되지 않는다. | D-02, D-03, D-04 |
| B3 | 원격도 같은 저장소의 여러 workspace를 Project로 묶고 하나의 workspace에 있는 서로 다른 checkout은 올바른 Project에 나누며 pane 없는 등록 프로젝트도 남는다. | D-03 |
| B4 | catalog의 로딩·비어 있음·오류·연결 끊김을 구별하고 마지막 목록은 stale로 표시하며, 실패나 root 미확인을 빈 목록이나 로컬 경로 결과로 대신하지 않는다. | D-03, D-12, D-14 |
| B5 | Explorer에서 폴더를 펼치고 파일명·내용을 검색하고 파일을 열 때 같은 UI를 사용하며 선택을 바꾸거나 검색을 취소하면 이전 결과가 현재 선택을 덮지 않는다. | D-05, D-14, D-15 |
| B6 | 목록 500개·검색 결과 80개·탐색 50,000개 상한을 넘으면 잘림과 범위 좁히기를 표시하고 숨은 무한 탐색을 계속하지 않는다. | D-15 |
| B7 | 열린 checkout 밖 경로, traversal, symlink 탈출, 검사 중 root 교체는 거절하며 remote 경로를 local filesystem에서 해석하지 않는다. | D-05, D-06 |
| B8 | 편집 가능 regular file은 16MiB까지 열고 초안을 유지하며, 권한 없음·비정규 파일·바이너리·초과 크기는 기존 읽기/다운로드 선택지와 함께 편집 불가로 구별한다. | D-05, D-06, D-13 |
| B9 | 같은 문서를 여러 진입점에서 열어도 한 버퍼를 공유하고 저장·dirty 상태가 일치하며 다른 기기의 동일 경로 편집과 섞이지 않는다. | D-04 |
| B10 | 기기 전환·연결 끊김·재접속은 열린 문서와 dirty 초안을 지우지 않고 마지막 읽은 revision 및 저장 가능 여부를 보여 준다. | D-04, D-14 |
| B11 | 기기 없는 기존 초안은 자동으로 현재 원격 기기에 귀속하지 않으며, 검증 가능한 기존 호스트에만 연결하고 나머지는 출처 미확인 복구 항목으로 열기·내보내기·명시적 폐기를 제공한다. | D-04 |
| B12 | 기존 초안 migration은 중단·재시도에 원본을 잃지 않으며, 선택하지 않은 기기를 닫힌 문서로 취급해 청소하지 않고 dirty 또는 출처 미확인 초안에는 14일 자동 폐기를 적용하지 않는다. | D-04, D-15 |
| B13 | 저장 전 다른 revision·파일 교체·쓰기 권한 변경을 발견하면 덮어쓰지 않고 충돌로 표시하며 내 초안 보존·최신본 확인·별도 내보내기로 복구한다. | D-06 |
| B14 | 저장 중 단절·timeout은 성공으로 표시하지 않고 결과 불명 상태를 남기며, 재접속 후 실제 파일 revision/내용을 확인해 저장됨·미저장·충돌로 판정하고 쓰기를 자동 재전송하지 않는다. | D-06, D-08, D-14 |
| B15 | 안전 저장을 증명할 수 없는 대상은 저장을 거절하고 초안을 내보낼 수 있으며, 저장 실패로 원본이 잘린 채 성공 표시되는 일은 없어야 한다. | D-06, D-12, D-20 |
| B16 | 새 파일·폴더, 이름 변경·이동은 선택한 checkout 안에서만 수행하고 기존 대상 덮어쓰기·이름 충돌·중간 경로 교체를 거절하며 실패 시 원래 위치와 편집 내용을 보존한다. | D-05, D-07 |
| B17 | 휴지통 동작은 기기와 대상 경로를 확인한 뒤 수행하며 검증한 파일과 실제 이동 대상이 달라지면 멈추고, 휴지통 미지원은 영구 삭제로 대체하지 않는다. | D-07, D-20 |
| B18 | 파일 이동·삭제 뒤 열린 문서는 실제 새 경로나 삭제 상태를 반영하고 dirty 내용은 보존하며, 부분 실패 시 남은 위치와 가능한 복구 행동을 알려 준다. | D-04, D-07 |
| B19 | Explorer의 변경 표시와 Git History/Changes는 같은 checkout의 같은 Git 상태를 보여 주며 branch diff와 uncommitted diff를 구별한다. | D-05 |
| B20 | uncommitted diff는 HEAD 대비 index·working tree를, branch diff는 확인한 base의 merge-base를 기준으로 읽으며 base 없음·Git 미지원·조회 실패를 깨끗한 저장소로 표시하지 않는다. | D-05, D-12 |
| B21 | rename·삭제·바이너리·untracked를 올바르게 구별하고 256KiB patch 상한에는 잘림을 표시하며, 단순 문자열 diff를 Git diff로 대신하지 않는다. | D-05, D-15 |
| B22 | 기기와 checkout을 바꾼 뒤 늦게 도착한 Git 결과는 새 화면에 섞이지 않으며 busy/실패/재조회 상태에서도 마지막 확인 결과와 freshness를 구분한다. | D-08, D-14 |
| B23 | 프로젝트 등록·pin·선택·find-or-create·파일 열기·탭 정렬·명시적 reopen은 양쪽 기기에 동일한 대상으로 동작하고 로컬에서만 실행되는 fallback이 없다. | D-03, D-08, D-09 |
| B24 | UI 등록은 해당 기기의 HOME 경계를 적용하고, 이미 CLI에서 등록한 외부 checkout은 기존 접근 계약으로 표시하며 UI 등록 권한을 조용히 넓히지 않는다. | D-03, D-07 |
| B25 | 프로젝트 등록 해제는 영향받는 pane을 확인한 뒤 닫고 성공 후 등록만 제거하며, 닫기 실패·timeout에는 등록을 유지하고 폴더나 worktree를 삭제하지 않는다. | D-08, D-10 |
| B26 | 기기 제거는 연결 설정과 표시를 제거하는 동작임을 보여 주고 원격 pane·agent·작업 디렉터리는 닫거나 지우지 않으며 남은 dirty 초안은 내보낼 수 있다. | D-04, D-10 |
| B27 | worktree 생성은 저장소·branch·경로를 검증하며 생성 성공 뒤 pane/agent 시작만 실패하면 생성된 worktree를 남기고 시작 재시도만 제공한다. | D-07, D-08 |
| B28 | worktree 제거는 대상 기기·경로·닫힐 pane을 확인하고 dirty/nested repository·HEAD·branch·base 및 등록 사실을 실행 직전에 재검증하며 force 삭제는 제공하지 않는다. | D-07, D-20 |
| B29 | branch 삭제는 별도 기본 꺼짐 선택이며 기존 안전 조건의 비강제 삭제만 허용하고, worktree 제거 성공·branch 보존/실패를 구별한다. | D-07, D-08 |
| B30 | purpose는 같은 한 줄 편집과 80 Unicode scalar 상한·40자 권장을 사용하며, 원격의 Herdr metadata와 로컬의 추가 Git branch description 저장 범위를 구별해 보여 준다. | D-11, D-21 |
| B31 | 닫기·split·zoom·resize는 Herdr의 실제 topology 결과로 확정하고 표시 탭·포커스 명령은 기존 core 소유권에 따르며 중복 topology를 웹에서 만들지 않는다. | D-08, D-09 |
| B32 | 명시적 reopen만 확인된 닫기 기록의 최근 20개 한도에서 복원을 요청하며, 기기 전환·재접속 자체가 agent 재실행이나 새 pane 생성을 일으키지 않는다. | D-08, D-09 |
| B33 | 같은 의도의 중복 입력은 진행 중 상태로 수렴하고 취소는 새 작업의 수락을 멈추며, 이미 발생한 외부 효과는 취소됐다고 숨기지 않고 실제 결과를 다시 확인한다. | D-08, D-14 |
| B34 | 작업 대상 기기·root·generation이 바뀌어도 이미 보낸 의도의 대상은 바뀌지 않으며 결과는 원래 작업에 귀속되고 현재 선택에는 적용되지 않는다. | D-08, D-14 |
| B35 | 외관·단축키·AI·기기 목록 설정은 hided 소유 호스트에 저장됨을 확인할 수 있고 원격 선택만으로 다른 호스트 설정을 수정하지 않는다. | D-11 |
| B36 | 선택 기기의 SSH/Auth/Herdr/Protocol/PTY/SFTP/Git 상태는 그 기기에서 확인한 사실만 보여 주며 다른 호스트의 hook/provider 상태를 대신 표시하지 않는다. | D-11, D-12 |
| B37 | 원격 hook 설치·지원하지 않는 설정에는 이유와 해당 호스트에서 설정하는 경로를 표시하며 자동 복사·권한 우회·숨은 runtime 설치를 하지 않는다. | D-11, D-12, D-20 |
| B38 | SSH는 기존 IdentityFile/agent와 known_hosts를 사용하고 인증·host-key 불일치를 구별하며 신뢰 검사를 우회하거나 비밀번호를 자동 입력하지 않는다. | D-12, D-20 |
| B39 | 파일 다운로드는 인증된 WS의 최대 4MiB 범위 전송과 256MiB 요청 상한을 지키며 대용량은 지원되는 저장 picker를 요구하고 무제한 메모리 Blob으로 우회하지 않는다. | D-13, D-15 |
| B40 | 파일 외부 열기는 호스트/클라이언트 신뢰 경계를 유지하고 실행 가능 파일은 실행하지 않으며, 원격 파일을 로컬의 같은 경로로 잘못 열지 않는다. | D-02, D-13 |
| B41 | 첨부는 사용자가 선택한 원래 pane·generation과 준비된 불변 bytes에 고정하며 기기 전환 후 다른 pane에 보내지 않고 재시도·취소에도 자동 Enter를 보내지 않는다. | D-13, D-14 |
| B42 | 첨부의 파일당 20MiB·배치 40MiB·8개, staging 128개·256MiB·24시간 만료와 private 권한, 대기 입력 64KiB 한도를 유지하고 초과 시 실행 전에 이유를 알린다. | D-13, D-15 |
| B43 | 원격 탐색·watch·전송은 소유 화면/연결이 끝나면 정리되고 watch/cache는 64개, 파일 쓰기 큐는 32개·128MiB 상한을 유지하며 포화 상태에서 새 요청을 조용히 버리지 않는다. | D-15 |
| B44 | 초안 저장 512MiB 또는 더 작은 브라우저 quota에 도달하거나 저장에 실패하면 이미 편집한 내용을 보존하고 새 편집·추가 로드를 제한하며 내보내기·명시적 정리를 제공하고 다른 기기의 미저장 초안을 자동 삭제하지 않는다. | D-04, D-15 |
| B45 | 요청 시작 시 busy, 완료 시 실제 결과, 실패 시 재시도·복구를 같은 제어 위치에서 제공하고 좁은 창·긴 한글/경로에서도 대상과 주요 행동을 읽을 수 있다. | D-12, D-16 |
| B46 | 공통 명령은 키보드 탐색·포커스 복귀·동일 tooltip/accessibility help를 유지하고 색상만으로 dirty·offline·disabled 상태를 구별하게 하지 않는다. | D-16 |
| B47 | 원격 I/O가 느려도 로컬 타이핑·탭 선택을 막지 않으며 동일 부하의 idle/driven 관측에서 기존 반응성 계약을 유지하고 고빈도 tick/tab마다 Git 프로세스를 만들지 않는다. | D-09, D-15, D-18 |
| B48 | 진단은 기기·작업 ID·실패 단계로 추적 가능하지만 파일 내용·초안·토큰·자격 증명을 기록하지 않으며 사용자가 조치할 수 없는 세부 오류는 화면 알림을 만들지 않는다. | D-08, D-16 |
| B49 | 양방향 호스트 선택, 동일 경로 격리, 저장 중 단절, 재접속 중복 의도, dirty 초안 복구까지 같은 사용자 흐름이 성립해야 하며 원격 기능 전체 비활성은 완료가 아니다. | D-01, D-18, D-21 |

## Technical structure

현재 로컬 catalog와 원격 raw session projection의 차이를 core의 기기별 catalog 계약으로 흡수한다.
Project 식별은 기존 resolver의 저장소/worktree 규칙을 유지하되 대상 호스트가 확인한 사실을 입력받고, UI의 이름과 경로는 권한이나 식별의 대체물이 되지 않는다.
문서 식별에는 기기·checkout·정규화된 문서 경로를 포함하고, 버퍼·초안 저장 키를 버전 migration하며 기존 root/path 키의 미확인 출처를 보존한다.
파일 서비스는 열린 root에 대한 confinement·revision·권한·제한된 전송·mutation 결과 계약을 소유하고, Git 서비스는 같은 root를 바탕으로 상태·base·diff를 소유한다.
현재 원격 SFTP 구현만으로 안전한 root-relative mutation과 저장 경합 방지가 증명되지는 않으므로 D-20 해소와 대상 측 안전 프로토콜 검증 전에는 쓰기 capability를 광고하지 않는다.
작성자 제안은 대상 측 최소 helper와 SSH 요청 수명이며, 설치·업데이트 정책은 미승인이고 새 상주 daemon이나 공개 파일 HTTP API를 도입하지 않는다.
원자적 파일 교체만으로 임의 외부 편집기에 대한 CAS를 주장하지 않으며, 지원 가능한 revision 경합 보장과 중단 복구를 명시하고 증명하지 못하는 경우 B13–B15대로 실패시킨다.
모든 명령은 공통 operation 수명과 authoritative 결과 확인을 통과하고 I/O·Git·직렬화는 Runtime lock 밖에서 수행하며 알림은 실제 상태 전이에만 발행한다.
Settings는 UI/daemon 소유 값, 선택 기기 진단, 명시적으로 허가된 대상 작업을 분리하고 연결 능력을 쓰기 권한으로 해석하지 않는다.
이관된 웹 로컬/원격 전용 orchestration과 불완전 FileService 경로는 공통 경계로 대체하며 Swift의 필수 병행 경로와 wire 단일 변환 경계는 보존한다.

## Risks

- **차단 결정, 사용자 소유:** D-20의 대상 호스트 helper 설치·갱신·실행과 확인된 원격 휴지통/worktree 삭제 허용 여부를 한 번에 결정해야 한다.
  이 결정 전 문서는 draft이며 implement를 시작하지 않고, 원격 읽기 성공을 쓰기 권한의 승인으로 해석하지 않는다.
- **기술 검증, 구현자 소유:** 외부 writer와의 경합·symlink/root 교체·연결 중단에 안전한 원격 저장 계약이 필요하다.
  단순 SFTP read-check-write나 atomic rename만으로 해결됐다고 주장하지 않으며 검증 실패는 parity 미완료로 보고한다.
- **이관 위험:** 기존 초안의 소유 기기는 경로만으로 복원할 수 없다.
  원본 보존과 미확인 복구 경로를 우선하고 중단된 migration을 재실행해도 중복 귀속·삭제가 없어야 한다.
- **검증 경계:** 이후 구현 검증은 두 호스트의 격리 checkout·임시 데이터·전용 Herdr server와 정확한 후보 PID/window에서 수행한다.
  운영 pane·서비스·설정·설치 앱에는 손대지 않고 실제 관측이 불가능한 경로는 미검증으로 남긴다.
- **후속·비차단:** 원격 hook/AI 설정 쓰기, purpose의 원격 Git description 복제, 새 탐색 레이아웃은 해당 소유권/화면 PRD에서 재검토한다.
- **문서 승인:** human_approval은 pending이며 완성본을 검토했다는 뜻으로 최신 구현 요청을 바꾸지 않는다.
  구현 시작 예외는 준비 완료 후 최신 사용자 문장을 그대로 기록하고 로컬 commit까지만 전달한다.
- **검사 범위:** S5.5 전용 qa-log가 없어 spec gate는 source 없음으로 생략하고 이전 roadmap qa-log를 새 권한의 근거로 쓰지 않는다.
  형식/readiness 통과와 PRD inline 의미 검토는 제품 구현·양방향 동작 검증의 PASS가 아니다.
