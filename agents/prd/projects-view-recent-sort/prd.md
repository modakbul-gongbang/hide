---
topic: "Projects View: 최근 작업 순 정렬"
status: "ready"
human_approval: "approved"  # user 2026-09-10 verbatim: ㅇㅇㅇ 그렇게 하자
review_profile: "standard"
review_rationale: "A read-only ordering and label change in one sidebar list plus a core sort fix; no data migration, credentials, or external effect beyond the pull request."
source_intake: "current conversation"
target_repository: "modakbul-gongbang/hide"
target_branch: "main"
created_at: "2026-09-10"
updated_at: "2026-09-10"
---

# PRD: Projects View: 최근 작업 순 정렬

## Goal

Projects View에 프로젝트가 많이 쌓여도 가장 최근에 작업한 프로젝트가 위에 오고, 각 행에 마지막 활동 시각이 보여서 지금 진행 중인 프로젝트를 오래 안 건드린 프로젝트 사이에서 바로 찾을 수 있다.
코어(`herdr-core/src/project_context.rs`)는 이미 프로젝트를 활동 순으로 정렬하지만(2026-09-09 PR #35), 행에 최근성이 보이지 않고, 원격 세션 경로는 여전히 이름순이며, 정렬을 고정하는 테스트가 없다.
이 Task는 task-factory 첫 end-to-end 파일럿(issue #2)이라 범위를 그 세 가지로 좁힌다.

## Non-goals

- 프로젝트 삭제·아카이브·숨기기·필터: 재검토는 이 정렬이 머지된 뒤 (D-04).
- Projects View 전면 재설계나 별도 View: 기존 사이드바 목록 안에서만 바꾼다 (D-01).
- 정렬 기준을 고르는 피커: v1은 활동 순 하나로 고정한다 (D-01).
- 기기(device) 그룹을 없애고 전 기기 통합 활동 순으로 섞기: 로컬/원격 구분은 유지한다 (D-02).

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 정렬은 코어가 이미 계산하는 활동 순(`project_context.rs` `sort_projects`)을 그대로 쓰고, 피커나 새 View 없이 기존 사이드바 목록에 적용한다. | issue #2 "별도 View인지 정렬 토글인지" 질문에 대한 최소안; `hide-chrome-and-tabs` PRD의 "순서 계산은 코어, 렌더링은 셸" 경계 |
| D-02 | "최근 작업"의 기준 = 코어 `Activity`(체크아웃의 마지막 커밋 시각과 에이전트 `last_activity` 중 최신). 기기 그룹 안에서만 활동 순이며 기기 그룹 순서는 유지한다. | 이미 코어에 있는 신호를 재사용; `hide-agent-attention` PRD의 정렬 원칙(최근 활동 내림차순) |
| D-03 | 원격 세션 목록(`session_sync.rs`)의 이름순 정렬을 같은 활동 순으로 바꿔 로컬과 원격이 같은 규칙을 따른다. | 같은 규칙이 두 경로에서 다르게 동작하면 사용자가 정렬을 신뢰할 수 없다 |
| D-04 | 각 프로젝트 행 끝에 마지막 활동의 상대 시각("3m", "2h", "5d")을 기존 `activityLabel` 자리에 표시하고, 활동 정보가 없으면 시각을 비운다. 에이전트 수 표시는 유지한다. | 정렬 근거가 보이지 않으면 사용자가 순서를 판단할 수 없음(design 4. 파생 상태를 보여준다); 코어 스냅샷에 시각 필드가 없어 셸까지 전달해야 한다 |
| D-05 | 정렬 규칙은 Rust 단위 테스트(`sort_projects`)와 Swift 프레젠테이션 테스트(상대 시각 라벨)로 고정한다. 기존 `scripts/rust-test.sh`, `scripts/swift-test.sh` 러너를 쓴다. | 현재 `sort_projects`에 전용 테스트가 없다; 정렬 회귀는 사용자가 늦게 발견한다 |
| D-06 | Delivery: hide의 sasu PR 모드(`agents/config.json`, base `main`, worktree, CI watch)로 Implementor가 PR을 열고 사람이 머지한다. | task-factory D-07 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | 사이드바 Projects 목록에서 같은 기기의 프로젝트는 마지막 활동이 최신인 것이 위에 온다. 활동이 같으면 기존 순서(id)를 유지한다. | D-01, D-02 |
| B2 | 프로젝트 안에서 커밋하거나 그 프로젝트의 pane에서 에이전트가 활동하면 다음 스냅샷에서 그 프로젝트가 자기 기기 그룹의 맨 위로 올라온다. | D-02 |
| B3 | 원격 세션의 프로젝트 목록도 같은 활동 순으로 보이며 더 이상 이름순이 아니다. | D-03 |
| B4 | 각 프로젝트 행 오른쪽에 마지막 활동의 상대 시각이 보인다(1분 미만 "now", 분·시·일 단위 한 토큰). 활동 기록이 없는 프로젝트는 시각 없이 기존 라벨만 보인다. | D-04 |
| B5 | 상대 시각은 앱이 열려 있는 동안 새 스냅샷마다 갱신되고, 라벨은 행의 이름을 밀어내거나 잘리게 하지 않는다. | D-04 |
| B6 | `scripts/rust-test.sh`에 활동 순·동률·활동 없음 케이스를 고정한 `sort_projects` 테스트가 있고, `scripts/swift-test.sh`에 상대 시각 라벨 테스트가 있다. | D-05 |

## Technical structure

- `herdr-core/src/project_context.rs`: `sort_projects`는 유지하고, 프로젝트별 최신 `Activity.unix_ms`를 `WorkspaceSnapshot`(navigator)에 `last_activity_unix_ms: Option<u64>`로 실어 보낸다(`model.rs`, `#[serde(default)]` 추가 필드, 스키마 버전 불변).
- `herdr-core/src/session_sync.rs`의 원격 목록 정렬을 같은 활동 키로 교체한다.
- `macos/Sources/HerdrMacOS/CoreBridge.swift` `CoreWorkspaceSnapshot`에 필드를 추가하고, `SidebarPresentation.swift` `activityLabel`에서 상대 시각을 합성해 `WorkspaceNavigatorRow`가 그린다. 상대 시각 포맷은 앱의 기존 시간 표기 헬퍼가 있으면 그것을 쓴다.
- 테스트: `herdr-core` 단위 테스트(`project_context.rs`), `macos/Tests/HerdrMacOSTests/SidebarPresentationTests.swift`.
- 새 설정, 영속 상태, 외부 API는 없다.

## Risks

- **신호 부재**: 커밋도 에이전트 활동도 없는 프로젝트는 활동 시각이 없어 그룹 맨 아래에 모인다. B4가 시각을 비워 이유를 드러낸다.
- **스냅샷 필드 추가**: Rust↔Swift 스냅샷 계약이 바뀌므로 hide PR 템플릿의 "snapshot wire" 질문에 답해야 하며, 필드는 additive여야 한다.
- **원격 경로 검증**: 원격 세션 정렬(B3)은 로컬 테스트로만 고정하고 실제 원격 관찰은 receipt에 한계로 적는다.
- **열린 결정**: 사람이 이 범위(가시적 최근성 + 원격 경로 일치 + 테스트)를 파일럿으로 승인해야 한다. 코어 정렬 자체는 이미 있으므로 "정렬만"으로는 변경할 것이 거의 없다.
