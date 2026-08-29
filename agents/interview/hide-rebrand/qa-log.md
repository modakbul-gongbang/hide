---
topic: "hide 리브랜딩·패키징·사이드바 재설계"
status: "active"
where: "brownfield"
selected_packs: "ux, compatibility, operation, verification, risk, provider"
created_at: "2026-08-29"
updated_at: "2026-08-29"
question_count: 0
normalization_policy: "transcript-sync-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: hide 리브랜딩·패키징·사이드바 재설계

## Current Understanding

- herdr-ide macOS 앱을 'hide'(herdr+ide)로 리브랜딩하고 패키징해 Spotlight 검색과 외부 사용자 배포가 가능하게 한다
- 좌측 패널을 사이드 패널로 재설계: Workspace 섹션(+worktree를 branch처럼), Agents 섹션(herdr-label처럼 상태·순서 유지, claude/codex favicon, 폴더 표시), 최하단 설정 아이콘 + 디바이스(local/mac mini) 연결
- Raycast DESIGN.md를 다운받아 루트에 넣고 AGENTS.md에서 참조시키며 전반적 디자인 리팩토링 진행
- 우측 파일트리는 선택된 워크스페이스에 맞춰 전환

## Intake Cursor

- next_decision_id: D-05
- next_question: (owned by the live conversation until checkpoint)
- last_materiality_sweep: preflight
- outstanding_raw_entries: none
- next_checkpoint_at: Q10

## Transcript Sources

| Runtime | Session ID | Start ref |
| --- | --- | --- |
| claude | ea9f12f2-8227-44ca-b8a9-96e2e5955ab7 | de5881ea-b24d-447e-8bc9-849888338af4 |

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | packaging | 현재 빌드는 macos/scripts/build_dev_app.sh가 debug 바이너리를 macos/build/assembled/HerdrIDE.app으로 조립하고 ad-hoc 서명(codesign --sign -)만 수행. /Applications 설치·Spotlight 노출·외부 배포(공증) 없음 | P0 | repo macos/scripts/build_dev_app.sh:6-30 | resolved |  |
| D-02 | fact | architecture | 앱은 --workspace-root 런치 인자로 단일 워크스페이스만 열며(CoreBridge.swift:338-355), 레이아웃은 HSplitView 3패널(AgentsPanel/TerminalPanel/WorkbenchPanel, ShellView.swift:28-58). 파일트리는 navigator.root_path 기반 WorkbenchPanel | P0 | repo macos/Sources/HerdrMacOS/ShellView.swift:28, CoreBridge.swift:338 | resolved |  |
| D-03 | fact | sidebar-data | SidebarAgent가 이미 workspaceLabel, agentKind(claude/codex), state, symbol, sortRank, elapsed, activity를 core snapshot으로 전달받음(CoreBridge.swift:117-141) - 사이드바 재설계에 필요한 데이터 대부분 존재 | P1 | repo macos/Sources/HerdrMacOS/CoreBridge.swift:117 | resolved |  |
| D-04 | fact | devices | mini 원격 워크스페이스 조회 코드가 이미 존재: ssh mini herdr workspace list를 호출해 RemoteWorkspaceSummary 리스트를 로드(OperationalModels.swift:396-475) | P1 | repo macos/Sources/HerdrMacOS/OperationalModels.swift:396 | resolved |  |

## Raw Q&A

## UX Scenario Cards

## Evidence From Code, Docs, Or Research

## Documented Domain Checks

- docs inspected:
- canonical terms:
- glossary or code conflicts:
- concrete scenarios tested:
- docs mutation:
- ADR candidate:

## Checkpoint And Sweep History

## Audit History
