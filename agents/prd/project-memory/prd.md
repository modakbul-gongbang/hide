---
topic: "Project Memory와 Session History"
status: "ready"
human_approval: "approved"  # user 2026-09-20 verbatim: $implement 이거 herdr sol xhigh로 해서 작업하게 시켜줘~ 검수도 꼼꼼히 ㄱㄱ
review_profile: "high-risk"
review_rationale: "로컬 Claude Code와 Codex 대화에서 파생 지식을 영구 저장하고, 로그인된 외부 provider로 세션 본문을 보내 분석하며, 두 runtime의 사용자 소유 hook 설정을 갱신해 이후 prompt에 context를 주입한다. 비밀값 유출, 잘못된 기억의 반복 주입, hook 지연, 사용자 설정 손상 위험 때문에 privacy, data lifecycle, runtime compatibility와 fail-open 동작을 높은 강도로 검토해야 한다."
source_intake: "agents/interview/project-memory/qa-log.md"
created_at: "2026-09-21"
updated_at: "2026-09-21"
---

# PRD: Project Memory와 Session History

## Goal

호연이 한 Project에서 오간 로컬 Codex와 Claude Code 세션을 Hide 안에서 한곳에 모아 찾아보고, 다시 설명하지 않아도 되는 결정과 규칙을 다음 작업에 자동으로 이어 쓰게 한다.
현재 Conversation Viewer는 실행 중인 pane 하나의 transcript만 보여 주며 프로젝트 전체 session catalog, 장기 Memory, prompt별 context 제공 내역은 소유하지 않는다.
이 기능은 오른쪽 panel에 네 번째 `Sessions` section을 추가하고 그 안에서 `Sessions / Memory`를 전환하게 한다.
Sessions는 두 runtime의 원본 기록을 provider가 드러나는 최신순 목록으로 보여 주고, Memory는 완료되거나 안정된 세션에서 추출한 재사용 가능한 프로젝트 지식을 자동으로 관리한다.

Project별 `Memory`를 한 번 켜면 Hide는 사용자가 이미 로그인한 Background AI provider를 통해 새 session 내용을 백그라운드에서 분석하고, 같은 의미를 병합한 활성 Memory를 로컬에 보존한다.
다음 agent session의 `SessionStart`에는 작은 Project Memory capsule을 제공하고, 각 `UserPromptSubmit`에는 현재 요청과 관련된 Memory만 로컬 검색으로 제공한다.
사용자 prompt 원문은 바꾸지 않고 Hide의 conversation ledger에만 `Memory attached N`을 표시하며, 이를 선택하면 어떤 Memory와 출처 session이 제공됐는지 확인할 수 있다.

성공의 기준은 자동화 자체가 아니라 신뢰 가능한 자동화다.
사용자는 Memory가 어디서 왔는지 확인하고, 잘못된 항목을 수정하거나 Forget하고, 최근 자동 학습을 Undo하며, Memory Off 또는 파생 데이터 삭제의 정확한 결과를 이해할 수 있어야 한다.
Hook 조회 실패, provider 장애, stale index는 agent 시작이나 prompt 제출을 막지 않아야 하며, 사용자가 조치할 수 있는 상태만 해당 session 또는 Memory pane에 표시한다.

## Non-goals

- `$remember`처럼 반복 교훈에서 프로젝트 문서, 규칙, 테스트 또는 코드를 자동으로 바꾸거나 개선 후보함을 만드는 기능은 이번 범위가 아니다.
- Hide는 Claude Code나 Codex가 소유한 원본 session 파일을 수정하거나 삭제하지 않는다.
- Hide는 Claude Code와 Codex의 자체 native memory 저장소를 읽거나 수정하거나 상호 중복 제거하지 않는다.
- Remote device의 session 동기화, remote runtime hook 설치와 local/remote Memory 동기화는 이번 범위가 아니다. 구현 전에 결정할 필요는 없고 remote Memory 기능을 열 때 다시 인터뷰한다.
- claude-mem, LangMem, Graphiti를 runtime dependency나 별도 user-managed service로 포함하지 않는다. Mem0 OSS는 Hide 내부 engine으로 사용하지만 사용자에게 engine 계정, API key, index 관리 또는 별도 daemon 운영을 요구하지 않는다.
- Mem0 밖에 별도 vector database나 cross-encoder reranker를 더하지 않는다. Hook의 100ms 경로는 Mem0가 background에서 만든 local search projection만 읽으며 prompt 제출 시 embedding 또는 model call을 하지 않는다.
- 사용자가 observation, embedding, merge, conflict, index 같은 내부 단위를 일상적으로 분류하거나 승인하는 관리 도구를 만들지 않는다.
- 모델이 제공된 Memory를 실제 reasoning에 사용했는지 추정하거나 `used`, `applied`라고 표시하지 않는다.
- 새 API key, Hide cloud account, 외부 Memory storage, cross-device sync를 요구하지 않는다.
- 원본 transcript의 전문 검색, 수정, export와 retention 설정은 이번 범위가 아니다. Sessions는 탐색과 읽기만 제공한다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | Memory는 전역 기본값이 아니라 Project별 opt-in이며 기본값은 Off다. | 인터뷰 D-02 |
| D-02 | 첫 제품 범위는 Sessions 탐색, Memory 자동 생성·관리, SessionStart와 UserPromptSubmit 주입까지다. 프로젝트 파일을 바꾸는 background improvement는 후속이다. | 인터뷰 D-05, D-12 |
| D-03 | 오른쪽 panel 순서는 `Overview · Explorer · History · Sessions`이고, Sessions section 안에서 `Sessions / Memory`를 전환한다. 선택은 기존 UI persistence에 포함한다. | 인터뷰 D-03, D-13; DESIGN.md의 현재 세 section 계약 |
| D-04 | 좁은 right panel은 목록, 검색, filter, 상태와 action을 담당하고, session transcript와 Memory 상세는 checkout당 기존 replaceable preview slot을 File·Diff와 함께 공유하는 중앙 preview tab으로 연다. Memory 편집을 시작하면 기존 preview 편집 규칙처럼 그 탭을 keep-open 상태로 승격한다. 선택한 항목이 이미 열려 있으면 그 tab을 focus하고, 현재 preview가 keep-open이면 active tab 다음에 새 replaceable preview를 만든다. | 인터뷰 D-13; 기존 editor-preview-tab 계약; engineering 2·7, design 5 |
| D-05 | Sessions 기본값은 Codex와 Claude Code가 섞인 최신순 목록이고 각 row에 provider badge를 그린다. 상단 `All / Codex / Claude Code` filter와 검색을 함께 사용할 수 있다. | 인터뷰 D-14 |
| D-06 | 사용자에게 보이는 Memory 한 항목은 다음 session에 단독으로 제공해도 의미가 통하고 독립적으로 수정·폐기할 수 있는 하나의 지속적인 프로젝트 사실, 결정, 규칙 또는 반복 방지 교훈이다. Raw session과 내부 observation은 출처다. | 인터뷰 D-18 |
| D-07 | 제안만 된 내용, 일시적 작업 상태, 한 번의 오류 문자열, 저장소에서 바로 읽을 수 있는 현재 코드 사실과 추측은 활성 Memory가 되지 않는다. Local redaction에서 credential 또는 secret candidate로 판정된 content는 Memory body, revision, provenance content와 Mem0 또는 FTS5 search projection 어디에도 저장하지 않는다. 같은 의미는 새 항목을 만들지 않고 provenance를 합친다. | 인터뷰 Q16, D-07, D-28; privacy 원칙 |
| D-08 | 새 일반 Memory는 출처와 함께 자동 활성화한다. 정정은 사용자가 Memory를 직접 Edit하고 Save한 경우 또는 Mem0가 direct human event를 source로 지정해 기존 item과 같은 subject에 대한 `supersedes` relation을 반환하고 Hide가 이를 검증한 경우에만 명시적으로 인정한다. 정정은 이전 revision을 superseded history로 남기고 replacement만 다음 injection에 제공하며 `Learned N memories · Undo`에서 되돌릴 수 있다. 출처만 다른 같은 의미는 merge한다. 이 조건을 만족하지 못해 어느 쪽이 authoritative인지 판정할 수 없는 충돌은 두 후보 모두 prompt 주입에서 제외하며 `Memory needs review`라는 최소 actionable state만 표시한다. Review에서는 기존 항목 유지, 새 항목으로 교체, 둘 다 Forget 중 하나를 고르고 그 결과만 future injection 대상이 된다. | 인터뷰 D-06, D-19, D-30; engineering 4·13, design 9·13 |
| D-09 | 기본 Memory 화면은 flat searchable list다. 내부 taxonomy와 상시 review queue를 숨기고, 상세에서 기억 내용, source sessions, learned timestamp, revision, `Provided to N sessions`, `Edit`, `Forget`만 보여 준다. | 인터뷰 D-19; design 1·2·3 |
| D-10 | 자동 학습 직후에는 modal 대신 `Learned N memories · Undo`라는 compact notice를 보여 준다. Notice를 8초 동안 유지하고 같은 app session의 Memory 목록에 최근 변경 표식을 남기는 동작은 측정과 native review로 조정할 수 있는 agent-owned reversible UX default이며 사용자 승인 product contract가 아니다. | 인터뷰 Q18; design 6·9 |
| D-11 | Project의 derived session source와 locator, cursor, provenance, Memory item과 revision, search index, injection receipt에는 자동 만료가 없고 Forget 또는 확인된 전체 삭제 전까지 보존된다. `Forget`은 선택한 Memory에 즉시 future injection에서 제외하는 tombstone revision을 만들고 같은 app session에서 Undo할 수 있다. `Delete Memory data`는 이 모든 Project 파생 데이터를 영구 삭제하므로 전체 범위를 설명하는 confirmation을 거친다. 두 동작 모두 raw provider session을 건드리지 않는다. | 인터뷰 D-07, D-26, Q20; design 6 |
| D-12 | Memory Off는 새 session 분석과 모든 새 injection을 즉시 중단하고 파생 데이터는 inactive 상태로 보존한다. 다시 On 하면 저장된 상태를 재사용하고 Off 동안 추가된 session만 catch up한다. | 인터뷰 D-07 |
| D-13 | Memory On 전에 현재 선택된 Background AI provider로 session 내용이 전송되고 해당 provider subscription usage가 발생할 수 있다는 점, provider의 자체 retention과 deletion terms가 적용되며 전송 뒤 Hide가 provider 보관본의 삭제를 보장할 수 없다는 점, 파생 Memory가 local disk에 유지된다는 점, credential은 Memory와 derived index에 저장하지 않는다는 점을 한 번 설명한다. 분석은 기존 `hide-ai`와 사용자가 로그인한 Claude Code 또는 Codex CLI만 사용한다. | 인터뷰 D-11, D-27, D-28; docs/AI_PROVIDERS.md |
| D-14 | Mem0 OSS를 automatic extraction, same-meaning deduplication, supersede/conflict planning과 search의 내부 engine으로 채택한다. `hide-memory`의 adapter가 durable Project ID, provenance, revision lifecycle, single-writer transaction, hook projection과 UI contract를 소유하고 Mem0 output은 직접 write authority를 갖지 않는다. | 사용자 Mem0 방향; 인터뷰 D-16, D-17; engineering 5·6·7·8 |
| D-15 | Memory store는 app-owned SQLite 한 개에 durable Project ID를 hard filter로 두고, Memory row, revision, provenance, session cursor, injection receipt와 Mem0 local search projection을 보관한다. Raw transcript body는 복사하지 않고 provider session locator, stable event offset와 content hash만 보존한다. | 인터뷰 D-07, D-17, D-24; data minimization |
| D-16 | Git linked worktree는 기존 canonical main worktree Project identity로 접는다. Plain folder와 device scope는 현재 core catalog identity를 재사용하며 session discovery, Memory store와 hook resolver가 같은 `hide-project` boundary를 사용한다. | 인터뷰 D-24; herdr-core `git_dir.rs`, `workspace.rs`; engineering 7·13 |
| D-17 | Mem0 search는 같은 Project의 active Memory만 대상으로 하고, `hide-memory`가 NFC normalization, Project hard filter, lifecycle exclusion, token budget과 source-diversity dedupe를 적용한다. Background에서 materialize한 local FTS5 projection은 2·3 character prefix와 bounded literal fallback을 제공해 Hook이 model 또는 embedding call 없이 deterministic lookup을 수행하게 한다. Ranking은 Mem0 relevance와 lexical score, salience, confidence, recency, cwd/path overlap을 입력으로 하고 superseded, tombstoned, conflicting item을 제외한다. | 사용자 Mem0 방향; 인터뷰 D-16; local retrieval research |
| D-18 | SessionStart는 사전 계산한 Project-wide capsule을 최대 5 items와 600 tokens 안에서 제공한다. UserPromptSubmit은 실제 prompt와 최대 최근 2개 human turn의 topic, current project/worktree metadata로 검색한 최대 3 items와 600 tokens를 제공한다. | 인터뷰 D-08, Q19; engineering 15 |
| D-19 | 두 injection 모두 whole item 단위로 중복을 제거한다. 예산을 넘으면 낮은 순위 item을 버리고 Memory text를 중간에서 자르지 않는다. SessionStart capsule과 같은 session에 이미 제공된 item은 prompt retrieval에서 제외한다. | 인터뷰 D-08; context integrity |
| D-20 | UserPromptSubmit hook은 model call, transcript scan, index write, process spawn을 하지 않는다. Read-only local lookup을 100ms hard deadline 안에 마치며 missing, locked, corrupt, stale 또는 over-deadline이면 빈 context와 success exit로 fail-open한다. | 인터뷰 D-08, D-17; docs/PERFORMANCE_TESTING.md; engineering 10·15 |
| D-21 | 사용자 prompt 원문은 그대로 둔다. Hide conversation ledger에서 N이 1 이상인 제출에만 `Memory attached N`을 표시하고, 선택하면 right panel Memory를 `This turn`으로 열어 provided items와 sources를 보여 준다. N이 0이면 아무 표시도 하지 않는다. | 인터뷰 D-15, Q19; design 4·10·13 |
| D-22 | SessionStart 제공 내역은 session detail에 `Project Memory ready · N`으로 표시한다. 모든 surface는 `attached` 또는 `provided`만 사용하고 `used` 또는 `applied`를 사용하지 않는다. | 인터뷰 Q19 |
| D-23 | Hook 설치와 갱신은 `hide-agent-hooks`만 수행한다. `UserPromptSubmit` 추가는 hook marker version을 올려 기존 설치를 outdated로 판정하고, Memory On에서 기존 Settings consent와 atomic config preservation 경로로 update를 요청한다. 거부 또는 실패하면 Memory는 On이 되지 않고 이유와 `Open Settings`를 보여 준다. | 인터뷰 D-10, D-21; docs/agent-hooks.md |
| D-24 | SessionStart output은 기존 worktree purpose instruction과 Memory capsule을 하나의 runtime-specific `additionalContext` envelope로 합성한다. `UserPromptSubmit` 등록 전에 실제 설치된 Codex와 Claude Code의 current hook schema를 fixture로 고정하고 둘 중 하나라도 지원하지 않는 버전은 그 runtime만 `Update required`로 둔다. | 인터뷰 D-21; compatibility research |
| D-25 | Background analysis는 기존 `hide-ai`의 process ownership, timeout, fallback과 duplicate suppression을 재사용하고 모든 성장 자원에 hard cap을 둔다. 초기 `max_in_flight=1`, `max_per_minute=30`, request input 64KiB, active Memory Project당 10,000 items, parsed hook stdin 256KiB는 대표 fixture 측정으로 조정할 수 있는 agent-owned implementation defaults이며 사용자 승인 product contract가 아니다. 측정으로 값을 바꿔도 cap 자체를 제거하거나 무제한 setting으로 바꾸지 않는다. | 인터뷰 D-25의 agent-owned assumption; engineering 14·15와 process practice |
| D-26 | `Stop` hook 또는 session file change 뒤 60초 동안 complete JSONL line이 더 생기지 않으면 unread cursor 이후 내용을 background analysis에 넣는다. 같은 provider/session/content hash의 retry는 같은 revision으로 converge하며, Hide가 꺼져 있던 동안의 기록은 다음 launch 또는 Memory On에서 newest-first로 catch up한다. | 현재 hook에 SessionEnd가 없다는 repository fact; engineering 11·14 |
| D-27 | Background request 전 known credential pattern을 local redaction하고 provider에는 필요한 normalized human/assistant event만 보낸다. Structured logs에는 request, project, session, memory IDs, counts, duration, outcome만 남기고 prompt, transcript, Memory text, file path, token과 provider thread ID는 남기지 않는다. | 인터뷰 D-07, D-11; docs/AI_PROVIDERS.md; engineering 9 |
| D-28 | 세션 row나 Memory pane에는 사용자가 조치할 수 있는 `Analysis paused`, `Hooks need update`, `Memory unavailable`, `Memory needs review`, capacity 상태만 가장 작은 형태로 표시한다. 자동 retry 중이거나 사용자가 조치할 수 없는 provider detail은 structured log와 Settings diagnosis에만 남긴다. | engineering 10, design 9·13 |
| D-29 | 원칙 intake는 `oh-my-principle` commit `654485f96b7764c759662d2c3e9e386ebc221cf6`을 기준으로 한다. Engineering 2·5·6·7·8은 Mem0 채택과 Hide adapter boundary, app-owned store, 기존 provider boundary 재사용으로, 4·10·11은 typed fail-open과 idempotence로, 9는 content-free logs로, 14·15는 worker ownership과 caps로 반영한다. Design 1·2·3·4·5는 list-preview와 one-toggle automation, 6은 Undo와 bulk-delete confirmation, 9·13은 minimal actionable states, 10은 produced counts only, 12는 실제 native width와 Korean/English 검증으로 반영한다. | `sasu principles list`; principles 문서 전문 |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | right panel section selector가 `Overview · Explorer · History · Sessions` 순으로 보인다. 기존 저장 상태는 그대로 읽고 새 설치는 Overview로 시작하며, 마지막 `Sessions / Memory` 선택은 Project별로 복원된다. | D-03 |
| B2 | Sessions 기본 목록은 현재 local Project의 Codex와 Claude Code 기록을 최신 activity 순으로 합친다. 각 row는 provider badge, 시작 또는 최근 시각, checkout/worktree, 첫 human request snippet과 unreadable 여부처럼 실제 parser가 생산한 값만 표시한다. | D-05, D-16 |
| B3 | `All / Codex / Claude Code` filter와 검색을 같이 적용해도 정렬은 유지된다. 필터 결과가 없으면 `No matching sessions`와 `Clear filters`를, 발견된 session 자체가 없으면 `No sessions yet`와 `Start agent…`를 보여 준다. | D-05; design 1·9 |
| B4 | session source 일부가 unreadable, moved 또는 malformed이면 읽을 수 있는 rows는 유지하고 affected row만 `Session unavailable`로 dim 처리한다. Row에서 Retry와 원본 위치 확인을 제공하며 전체 목록을 failure screen으로 바꾸지 않는다. | D-28; design 9·13 |
| B5 | session 또는 Memory row 한 번 클릭은 checkout의 replaceable preview slot에 read-only detail을 연다. 선택한 항목이 이미 열려 있으면 그 tab을 focus한다. 현재 preview가 replaceable이면 같은 slot을 교체하고, 현재 detail이 keep-open이면 그 tab은 보존한 채 active tab 바로 다음에 새 replaceable preview를 열어 선택 결과가 사라지지 않게 한다. | D-04 |
| B6 | transcript detail은 기존 provider-neutral ledger formatting을 재사용하고 human, assistant, injected context를 구분한다. Memory가 제공된 turn에는 prompt 아래 `Memory attached N`이 보이지만 hook context 전문은 transcript message인 것처럼 섞이지 않는다. | D-04, D-21 |
| B7 | Memory가 Off인 Project에서 Memory tab을 처음 열면 기능 설명, 선택된 provider로의 session 전송과 subscription usage 가능성, provider retention과 deletion terms 적용, 전송 뒤 Hide가 provider 보관본 삭제를 보장할 수 없다는 점, local persistence와 secret exclusion을 보여 주고 primary action 하나 `Turn on Memory`를 제공한다. 내부 engine이나 index 용어는 보이지 않는다. | D-01, D-09, D-13 |
| B8 | current hooks가 Memory events를 지원하지 않으면 `Turn on Memory` 다음에 수정 대상 config 파일과 보존 범위를 설명하는 `Update agent hooks` confirmation이 열린다. Cancel은 아무 파일도 바꾸지 않고 Memory를 Off로 유지한다. | D-23 |
| B9 | hook update와 Memory enable이 성공하면 기존 session backfill 수, analyzed 수, failed 수가 `Analyzing 14 of 38 sessions`처럼 보이고 Sessions 탐색은 계속 가능하다. 앱 재시작 후 같은 cursor에서 이어지며 이미 처리한 content를 다시 AI에 보내지 않는다. | D-25, D-26 |
| B10 | provider가 unavailable, logged out, usage-limited 또는 over-budget이면 자동 학습은 `Analysis paused`로 멈추고 이미 저장된 Memory 검색과 Sessions 탐색은 계속된다. Action은 상태에 맞는 `Open Settings`, `Sign in` 또는 `Retry` 하나만 제공한다. | D-13, D-28 |
| B11 | backfill 중 일부 session만 실패하면 `36 analyzed · 2 failed`를 표시하고 failed sessions만 Retry한다. 성공한 session과 Memory를 rollback하거나 중복 분석하지 않는다. | D-26; engineering 10·11 |
| B12 | background analysis가 durable fact, decision, rule 또는 repeat-prevention lesson을 찾으면 새 active Memory를 만들거나 같은 의미의 기존 item에 source를 추가한다. 한 session 요약이나 raw 대화 조각을 Memory row로 그대로 만들지 않는다. | D-06, D-07 |
| B13 | Memory의 Edit 후 Save 또는 direct human source가 연결된 검증 가능한 `supersedes` 결과만 명시적 정정으로 처리한다. 기존 item은 superseded revision history에 남고 replacement만 다음 injection에 들어가며 자동 정정이면 `Learned N memories · Undo`에 포함된다. Ordinary discussion이나 source가 불명확한 상반된 후보는 정정으로 추측하지 않고 모두 자동 주입에서 제외한 뒤 Memory tab 상단에 `1 memory needs review` 한 줄과 Review action을 표시한다. Review는 `Keep existing`, `Replace with new`, `Forget both`를 제공하고 선택 결과만 future injection에 포함한다. | D-08 |
| B14 | 정상 상태의 Memory tab은 flat list와 검색, `Memory on`, active count만 표시한다. 각 row의 실제 기억 문장, source count와 updated time을 읽을 수 있고 category, observation count, vector 상태, merge queue는 없다. | D-09 |
| B15 | Memory row를 클릭하면 중앙 preview에 full content, Codex와 Claude Code source sessions, learned time, revision history, `Provided to N sessions`가 열린다. Source를 선택하면 해당 session preview로 이동한다. | D-04, D-09, D-22 |
| B16 | `Edit`을 누르면 Memory preview tab이 keep-open으로 승격되고 명시적 Save와 Cancel을 제공한다. Save는 새 revision을 만들고 FTS index를 같은 transaction에서 갱신하며, Cancel은 아무 state도 바꾸지 않는다. | D-04, D-14 |
| B17 | 자동 학습이 끝나면 `Learned 1 memory · Undo` notice가 작업을 가리지 않고 나타난다. Undo는 그 batch가 만든 revision만 되돌리고 다른 session의 이후 변경을 덮어쓰지 않는다. | D-10; engineering 11 |
| B18 | `Forget`은 선택한 item을 즉시 목록에서 inactive 처리하고 future injection에서 제외하며 같은 app session에 Undo를 제공한다. 이미 기록된 `Provided to N sessions` receipt는 역사적 provenance로 남는다. | D-11 |
| B19 | `Turn off Memory`는 확인 modal 없이 즉시 분석과 injection을 멈추고 `Memory off · 42 memories kept`를 보여 준다. 다시 켜면 기존 item을 복원하고 Off 이후 session만 분석한다. | D-12 |
| B20 | `Delete Memory data…`는 derived session sources와 locators, cursors, provenance, active와 inactive Memory, revisions, Mem0/search projection, injection receipts가 삭제되고 raw Codex와 Claude Code sessions는 남는다는 confirmation을 보여 준다. Confirm 뒤 Memory는 Off와 empty가 되고 source link, analysis progress와 provided history도 사라지며 이 삭제에는 Undo가 없다. | D-11 |
| B21 | Memory가 On인 새 session의 SessionStart는 가장 중요한 active Memory를 최대 5개, 600 tokens 안에서 제공한다. Session detail에는 실제 N만으로 `Project Memory ready · N`을 표시하며 N이 0이면 표시하지 않는다. | D-18, D-22 |
| B22 | 사용자가 prompt를 제출하면 local read-only retrieval이 current Project와 prompt에 관련된 active Memory를 최대 3개, 600 tokens 안에서 제공한다. 화면의 prompt 문자열과 provider composer history는 원래 입력 그대로다. | D-18, D-20, D-21 |
| B23 | 제공한 Memory가 있으면 Hide conversation ledger의 해당 prompt 아래 `✦ Memory attached 2`가 보인다. 선택하면 Memory tab이 `This turn` filter로 열리고 item, revision과 source session을 보여 준다. | D-21 |
| B24 | 관련 Memory가 없거나 Memory가 Off이면 hook은 context를 추가하지 않고 indicator도 그리지 않는다. Zero를 `Memory attached 0`으로 표시하지 않는다. | D-12, D-21 |
| B25 | FTS candidate가 3개 또는 600 tokens를 넘으면 rank가 낮은 whole item부터 빠진다. 같은 item, superseded revision, conflict, tombstone, SessionStart에서 이미 제공한 item은 중복 제공되지 않는다. | D-17, D-18, D-19 |
| B26 | hook DB가 missing, locked, corrupt, stale 또는 100ms를 넘으면 agent start와 prompt submit은 Memory 없이 계속된다. 해당 turn은 `Memory unavailable` receipt를 가질 수 있지만 raw storage detail은 UI copy가 아니라 diagnosis에 남고 Retry는 다음 prompt retrieval만 다시 시도한다. | D-20, D-28 |
| B27 | current runtime version이 UserPromptSubmit context injection을 지원하지 않으면 그 runtime row와 Settings에 `Update required`가 보이고 해당 runtime은 injection하지 않는다. 지원되는 다른 runtime과 Sessions catalog는 계속 작동한다. | D-24 |
| B28 | hook input이 256KiB를 넘거나 Project identity를 안전하게 resolve하지 못하면 context를 제공하지 않고 prompt는 계속된다. 다른 Project의 Memory로 fallback하지 않는다. | D-16, D-20, D-25 |
| B29 | Memory item cap에 도달하면 새 item을 자동 삭제하거나 cap을 늘리지 않는다. 기존 retrieval은 유지하고 `Memory capacity reached`와 `Review memories` action을 보여 주며 같은 의미 merge와 supersede는 계속 허용한다. | D-25 |
| B30 | session analysis 전에 known secret pattern이 local redaction되고 secret 또는 credential candidate는 Memory body, revision, provenance content, Mem0 또는 FTS5 projection에 저장되지 않는다. Diagnosis와 AI logs 어디에도 prompt, transcript 또는 Memory body가 없다. | D-07, D-27 |
| B31 | 앱을 종료하면 background analysis owner와 provider child가 기존 hide-ai shutdown 경로로 끝나고 pending cursor는 durable state로 남는다. 다음 launch에서 orphan process 없이 resume한다. | D-25, D-26; process practice |
| B32 | 320pt, 344pt, 400pt right-panel 폭에서 `Sessions / Memory`, mixed Korean-English Memory 문장, provider badge, counts와 actionable state가 잘리지 않는다. Long snippet은 tail truncate하고 full text는 tooltip과 accessibility label에 남긴다. | D-03, D-09, D-29; design 12 |
| B33 | VoiceOver는 session row를 provider, first request, checkout, time, availability 순으로 읽고, Memory row를 content, source count, status 순으로 읽는다. `Memory attached N`과 모든 icon action은 화면 label과 동일한 accessibility name/help를 가진다. | D-21, D-29 |
| B34 | deterministic tests는 provider-neutral discovery, canonical Project folding, cursor restart, duplicate convergence, lifecycle transitions, transactionally consistent FTS, ranking caps, exact token/item exclusion, fail-open hook envelopes, config preservation, redaction과 content-free logs를 외부 observable outcome으로 고정한다. | D-14~D-27; engineering 11·12 |
| B35 | Swift presentation tests는 네 번째 section order와 persistence migration, filter combinations, empty/no-results/partial/failure/off/on states, exact `attached/provided` copy, zero-count omission, preview promotion과 accessibility output을 값으로 검증한다. | D-03~D-05, D-21, D-28; design 9·10 |
| B36 | native acceptance는 worktree-local signed dev bundle, isolated Herdr server와 private app state에서 실제 Codex와 Claude Code fixture session을 함께 보여 준다. Memory On, backfill, source navigation, edit/Forget/Undo, Off/Delete, SessionStart, prompt attachment, no-match와 fail-open을 screenshot과 interaction으로 확인하고 operator app과 server는 건드리지 않는다. | docs/PERFORMANCE_TESTING.md; AGENTS.md Desktop App Verification |
| B37 | hook path 측정은 100ms deadline, no child process, no DB write, bounded candidate count와 no prompt blocking을 확인하고 idle과 driven 결과를 따로 기록한다. Background analysis의 request, queue, descendants와 RSS가 기존 hide-ai caps 안에 있는지도 별도로 기록한다. | D-20, D-25; performance guide |

## Technical structure

`hide-session`이 raw session discovery와 provider-neutral parsing의 유일한 owner다.
기존 단일-session locator와 incremental reader를 project-wide enumerator, catalog metadata와 durable provider/session cursor까지 확장한다.
Claude Code와 Codex JSONL을 terminal scrape나 새 subprocess로 읽지 않으며, complete normalized events만 background analysis에 넘긴다.
Archived detail은 기존 `ConversationLedgerView`의 Markdown과 turn presentation을 재사용하되 live `ConversationPaneView`, PTY fallback, `conversation_pane_ids`와 `SidebarAgent` 의존성은 재사용하지 않는다.

새 `hide-project` boundary는 cwd, linked worktree, plain folder와 device-scoped core identity를 하나의 durable Project key로 resolve한다.
`herdr-core` catalog, `hide-session` enumeration, `hide-memory` store와 hook helper는 이 resolver만 사용하고 각자 Git root 규칙을 다시 구현하지 않는다.
Resolution 실패는 다른 Project 또는 display name으로 fallback하지 않는 typed result다.

새 `hide-memory` crate는 pinned Mem0 OSS dependency를 감싸는 adapter, app-owned SQLite database, schema migrations, local search projection, retrieval와 `MemoryWriteService`를 소유한다.
Mem0는 automatic extraction, same-meaning deduplication, relation planning과 search를 제공한다.
Hide adapter는 Mem0의 LLM 요청을 기존 `hide-ai`로, persistence를 app-owned SQLite로, project filter와 lifecycle을 domain command로 연결하므로 별도 API key, 외부 Mem0 account 또는 user-managed daemon이 필요하지 않다.
정규 table은 `projects`, `session_sources`, `session_cursors`, `memory_items`, `memory_revisions`, `memory_sources`, `injection_receipts`를 갖고, FTS5는 active revision의 title, body와 normalized terms만 index한다.
`memory_items`는 lifecycle state, current revision, confidence, salience와 timestamps를, provenance는 provider/session/event offset/hash를, receipt는 runtime/session/turn과 provided memory IDs만 저장한다.
Write service는 transaction 안에서 revision, lifecycle, provenance와 FTS를 함께 갱신하고 schema version이 맞지 않으면 read/write를 중단해 migration error를 올린다.
Hook process와 renderer는 read-only connection만 사용하며 foreign key, integrity check 또는 FTS drift가 실패하면 rebuild를 enqueue하고 fail-open한다.

Mem0 adapter는 `hide-ai`에 feature id, request id, Project/session subject id, redacted normalized events, strict output schema와 deadline을 제출한다.
Adapter schema는 candidate text, kind, confidence, source offsets, relation(`new`, `same`, `supersedes`, `conflicts`, `discard`)을 반환하고 feature layer가 이를 Memory domain command로 검증한다.
Mem0와 provider result는 write authority를 갖지 않으며 invalid output은 hide-ai의 기존 bounded retry 뒤 typed failure가 된다.
Background coordinator는 core worker context가 소유하고 process나 timer를 별도로 상주시하지 않는다.
한 Project와 session cursor에 동시에 하나의 analysis intent만 허용하고 owner drop은 inflight work를 취소하거나 result를 무시한 뒤 provider child를 기존 shutdown 경로로 끝낸다.

Retrieval은 `MemoryRetriever` interface 뒤에서 Mem0 search와 local FTS5 projection을 결합한다.
Background analysis와 app-side search는 Mem0 relevance 결과를 Hide-owned Project, lifecycle, provenance와 budget rules로 검증하고, 같은 active revision 집합을 FTS5 projection에 transactionally materialize한다.
SessionStart는 write service가 active revisions로 미리 만든 capsule snapshot을 읽고, UserPromptSubmit hook은 normalized prompt terms, 최근 두 human topic, cwd/path terms로 local projection에서 최대 60 candidates를 조회한 뒤 hard filters, deterministic rank, source-diversity dedupe와 token budget을 적용한다.
따라서 prompt path에는 Mem0 process, embedding, model 또는 network call이 없고 Mem0 search semantics의 local projection만 소비한다.

`hide-agent-hooks`는 `UserPromptSubmit` event, runtime별 bounded stdin parser, Project resolver와 read-only retriever를 추가한다.
Install marker를 올리고 existing install/status/diagnosis/atomic-write tests를 확장하며 다른 도구의 hook entries와 ordering을 보존한다.
SessionStart는 기존 purpose instruction과 capsule을 한 envelope로 만들고, UserPromptSubmit은 runtime별 verified `additionalContext` envelope와 receipt를 만든다.
Helper는 모든 outcome에서 stdin을 안전하게 drain하고 exit zero를 유지하며, `hide-ai`, Claude, Codex, Herdr CLI 또는 다른 child를 시작하지 않는다.

`herdr-core`는 네 번째 `RightPanelSection::Sessions`, 내부 Sessions/Memory mode, filters, selected item, preview tab payload, Memory enablement, progress, actionable state와 attachment receipts를 snapshot authority로 소유한다.
한 user action은 한 typed event다. `memory_open_for_turn`은 panel visibility, Sessions section, Memory mode와 This turn filter를 한 frame에 바꾼다.
SQLite, filesystem, AI, hook config와 JSON serialization은 `Mutex<Runtime>` 밖에서 실행되고 worker result만 lock 안에서 state transition을 적용한다.
Notifier는 실제 state transition에만 한 번 announce한다.

Swift shell은 snapshot을 그리며 provider/session parsing, ranking, count 추론 또는 failure classification을 하지 않는다.
Right panel은 existing `HideChoiceGroup`, `HideSearchField`, `HideBadge`, `HideTextButtonStyle`, `HideEmptyState`, command tooltip와 `HideTheme` tokens를 사용한다.
Session과 Memory detail은 새 editor tab kinds로 기존 replaceable preview lifecycle, strip identity, close, focus, reorder와 Recent Panels contract를 확장한다.
새 visual token이나 reusable control이 필요하면 먼저 `HideTheme`, `DESIGN.md`, `design/hide-ui.lib.pen`의 승인된 master와 `pen-token-map.json`을 함께 갱신하며 task screen은 library에 넣지 않는다.

구현 변경은 `docs/README.md` ownership map, `docs/ARCHITECTURE.md` core/worker/store boundary, `docs/agent-hooks.md` events, envelopes, install version과 failure, `docs/AI_PROVIDERS.md`의 Memory feature consumer, `docs/PERFORMANCE_TESTING.md`의 hook cost contract와 `DESIGN.md`의 four-section Sessions/Memory UI 계약을 같은 PR에서 갱신한다.
Raw run evidence, screenshots, provider outputs와 session fixture captures는 `agents/runs/project-memory/`에만 둔다.
Delivery는 `agents/config.json`의 `main` base, worktree, pull-request, CI watch 계약을 따르되 이 PRD 생성 단계에서는 구현, push와 PR을 수행하지 않는다.

## Risks

- Privacy와 provider egress가 가장 큰 위험이다. Memory On disclosure가 분석 provider, provider-side retention과 deletion limitation, local retention을 먼저 설명하고, local redaction, Project hard filter, content-free structured logs, raw transcript 비복제로 노출을 줄인다. Redaction은 알려진 pattern에 대한 best effort이고 Hide는 provider에 이미 전달된 content 삭제를 보장할 수 없으므로 사용자가 provider 전송 자체를 허용하지 않으면 Memory를 켜지 않아야 한다.
- 잘못되거나 오래된 Memory가 반복 주입되면 한 번의 나쁜 답보다 피해가 커진다. Provenance, revision, edit, Forget, Undo, explicit correction supersede, ambiguous conflict exclusion과 hard token/item caps로 오염 범위를 제한한다.
- Hook이 prompt path를 지연하거나 깨뜨릴 수 있다. 100ms read-only deadline, no model/process/write, exit-zero fail-open, input cap과 per-runtime schema fixture를 acceptance boundary로 둔다. Runtime이 hook contract를 바꾸면 injection만 unavailable로 두고 agent와 Sessions를 계속 쓴다.
- 사용자 소유 Claude Code와 Codex config를 손상할 수 있다. 외부 config writer는 `hide-agent-hooks` 하나뿐이고 atomic rename, marker version, other-entry preservation, temp HOME regression test와 explicit update consent를 유지한다.
- SQLite corruption, migration 또는 FTS drift가 다른 Project 지식을 섞을 수 있다. Durable Project ID hard filter, transactional revision/index update, startup integrity/version check, no cross-project fallback과 rebuildable index를 사용한다. Canonical rows를 읽을 수 없으면 prompt를 block하지 않고 retrieval을 중단한다.
- Background analysis가 subscription usage, CPU, process와 disk를 끝없이 늘릴 수 있다. 기존 hide-ai request/process caps, one inflight analysis, resumable cursor, bounded request input과 active item count, caller-visible capacity failure를 둔다. 초기 64KiB request와 Project당 10,000 active items는 측정으로 조정할 수 있는 agent-owned implementation defaults다. Cap을 넘었다고 자동 삭제하거나 자동으로 더 큰 값으로 바꾸지 않는다.
- Mem0 upgrade가 extraction, merge 또는 search semantics를 바꾸면 기존 Memory가 조용히 달라질 수 있다. Dependency version과 adapter schema를 pin하고 representative Korean/English fixtures로 output class, idempotence, Project isolation과 projection parity를 검증한 뒤에만 올린다.
- `Stop`은 session end가 아니라 turn end일 수 있다. 60초 quiescence와 complete-line cursor, content hash dedupe로 partial JSONL과 repeated analysis를 피하고 next launch catch-up으로 missed hook을 복구한다.
- Session이나 source file이 이동 또는 삭제될 수 있다. Memory는 stable IDs와 source offset/hash를 유지하고 detail은 source unavailable을 표시하지만 active Memory를 조용히 삭제하지 않는다. 사용자가 Memory 자체를 Forget하거나 bulk delete할 때만 lifecycle이 바뀐다.
- 기존 preview slot에 Session과 Memory를 추가하면 File/Diff navigation을 깨뜨릴 수 있다. 한 checkout 한 slot, read-only replacement, edit-time promotion, dirty protection, close/reorder/Recent behavior를 기존 editor preview tests와 새 cross-kind tests로 함께 고정한다.
- Native UI는 Pen mock만으로 승인할 수 없다. 320pt, 344pt, 400pt panel width, Korean/English wrapping, VoiceOver, empty/partial/error states와 exact app PID/window를 isolated signed bundle에서 screenshot으로 확인하고 human visual review가 남으면 구현 완료로 부르지 않는다.
- 현재 사용자 결정이 필요한 구현 전 항목은 PRD 자체 승인뿐이다. Remote Memory 범위는 명시적으로 deferred이며 V1을 막지 않는다.
