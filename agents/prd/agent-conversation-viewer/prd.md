---
topic: "Agent Conversation Viewer"
status: "ready"
human_approval: "pending"
review_profile: "standard"
review_rationale: "macOS shell과 Rust core에 읽기 전용 대화 보기와 로컬 세션 파일 읽기를 추가하는 사용자 화면 변경이며, 외부 데이터 쓰기나 인증·결제 변경은 없다."
source_intake: "agents/runs/agent-conversation-viewer/design/brief.md"
created_at: "2026-09-15"
updated_at: "2026-09-16"
---

# PRD: Agent Conversation Viewer

## Goal

운영자가 Claude 또는 Codex가 실행 중인 pane에서 터미널 출력과 도구 과정을 숨기고, 실제 요청과 답변만 읽을 수 있는 Conversation Viewer를 사용할 수 있게 한다.

Viewer는 pane 안에서 터미널과 토글되며, 터미널은 계속 실행되고 입력 가능한 복구 경로로 남는다.

요청과 답변은 로컬 agent session JSONL에서 읽어 native selectable Markdown surface로 보여 주고, 새 turn은 사용자 입력 경계에 맞춰 기존 assistant 출력 블록에 추가한다.

지원되는 agent pane은 Conversation Viewer를 기본 모드로 열고, 운영자는 각 pane의 header 버튼 또는 rebind 가능한 pane command로 terminal로 전환할 수 있다.

이번 PRD의 화면 목표는 운영자 승인을 받은 ledger 방향을 `design/hide.pen`의 `Screen / Pane / Conversation - ...` 보드 여섯 개로 고정하는 것이다.

## Non-goals

- Viewer에서 새 prompt를 입력하거나 전송하지 않는다.
- Viewer에서는 tool call, tool result, thinking, progress, system reminder, local-command echo를 원문으로 보여 주지 않는다.
- 새 provider API, 서버 transcript 저장소, 계정 연동, 인증, 결제, 데이터베이스 또는 외부 네트워크를 추가하지 않는다.
- Herdr의 PTY, agent lifecycle, header status, pane topology, zoom 동작을 바꾸지 않는다.
- 기존 agent-context-labels 플러그인의 사용자 화면이나 token 계약을 바꾸지 않는다.
- transcript를 Hide가 별도 캐시 파일로 복제하지 않는다.
- Browser pane이나 HTML viewer를 제품 화면으로 추가하지 않는다.
- 지원하지 않는 provider나 agent session을 추측해서 대화 화면으로 만들지 않는다.
- 사용자가 명시적으로 요청하지 않은 기존 dirty worktree, 설치된 앱, 실행 중인 pane을 정리하거나 재시작하지 않는다.

## Decisions

| D-n | 결정 | 근거 |
| --- | --- | --- |
| D-01 | 화면은 chat bubble, avatar, card가 없는 transcript ledger로 구현한다. | 승인된 design handoff와 `design/hide.pen`의 새 Screen 보드가 확정한 시각 방향이다. |
| D-02 | pane header에서 Conversation toggle은 Zoom 앞에 있는 `HideIconButton`이며 Conversation 선택 중에는 selected 상태를 보인다. | 승인된 handoff 결정 1이며 기존 pane header와 icon button 패턴을 재사용한다. |
| D-03 | mode 전환 command의 이름은 `toggle_conversation`이고 기본 단축키는 `⌘⌥C`이다. | `PaneShortcutSettings`의 pane command로 선언해야 하며 Settings에서 rebind 가능해야 한다. `⌘⌥↩`은 Zoom이므로 사용하지 않는다. |
| D-04 | 시간 표시 토글과 `toggle_conversation_times` command는 제공하지 않는다. Conversation Viewer에서는 clock과 elapsed rail을 항상 표시한다. | 운영자 결정 변경. 시간 정보를 별도 조작 없이 기본 맥락으로 제공한다. |
| D-05 | 지원되는 agent pane의 Conversation mode는 pane별 ephemeral UI state이며 초기값은 Conversation이다. 사용자가 terminal로 전환한 상태는 기존 pane lifecycle 동안만 유지한다. | 운영자 결정 변경과 `CoreUIStateSnapshot`/`UiState`의 기존 책임 경계를 따른다. |
| D-06 | Conversation은 읽기 전용이고 PTY는 계속 실행되며 header의 agent status는 바꾸지 않는다. | 승인된 handoff 결정 5이며 터미널을 숨기는 것이 agent 실행을 멈추는 의미가 아니기 때문이다. |
| D-07 | 질문, 승인, 오류처럼 운영자의 입력이 필요한 상태에서는 pane 전체를 terminal fallback으로 전환하고 기존 pane notice row를 사용한다. | 승인된 handoff 결정 6의 confirmed decision을 따른다. 정적 HTML 시안에서 notice 아래에 transcript가 남아 있던 부분보다 brief의 결정 표를 우선한다. |
| D-08 | demand fallback이 해제되면 사용자가 fallback 직전에 선택했던 Conversation 또는 terminal mode로 돌아간다. | D-07의 복구를 예측 가능하게 만드는 구현 가정이다. 현재 요구를 충족하며 별도 저장 형식 없이 되돌릴 수 있다. |
| D-09 | 사용자가 직접 입력한 text만 user turn이고, assistant turn은 provider session의 assistant text만 표시한다. | 승인된 handoff 결정 9이며 도구와 내부 진행 상황을 답변으로 오인하지 않게 한다. |
| D-10 | Claude와 Codex는 같은 layout을 사용하고 provider 차이는 header identity와 prompt glyph에만 반영한다. Claude glyph는 `>`, Codex glyph는 `›`이다. | 승인된 handoff 결정 10과 기존 provider artwork/identity 컴포넌트를 따른다. |
| D-11 | Claude session은 `~/.claude/projects/<slug>/<session-id>.jsonl`, Codex session은 `~/.codex/sessions/<YYYY>/<MM>/<DD>/rollout-*.jsonl`에서 찾고 pane의 `agent_session`을 source key로 사용한다. | 승인된 handoff 결정 10과 현재 `LocalSessionReader`의 로컬 경로 계약을 재사용한다. |
| D-12 | session file read, tailing, parsing, refresh는 `Runtime` mutex 밖에서 수행한다. core snapshot은 mode를 전달하고 shell/local reader가 transcript payload를 그린다. | AGENTS.md의 runtime architecture와 engineering 원칙 5, 7에 따른 경계다. blocking I/O와 큰 serialization은 lock 안에 둘 수 없다. |
| D-13 | 각 turn의 source clock을 64pt rail에 9pt mono muted로 보이고, elapsed를 10pt mono secondary로 보인다. 진행 중인 assistant turn은 blue `●`와 live elapsed를 사용한다. | 운영자 결정 변경. 시간 정보는 항상 표시한다. |
| D-14 | elapsed는 대응하는 user prompt의 source timestamp부터 assistant turn의 마지막 표시 가능한 assistant event timestamp까지로 계산한다. 진행 중인 turn은 마지막 prompt timestamp부터 현재 시각까지 계산하고 숨긴 tool event도 그 시간에 포함한다. | 정적 시안의 turn rail을 재현하기 위한 구현 가정이며 timestamp가 없는 event는 화면에 숫자를 만들지 않는 원칙으로 처리한다. |
| D-15 | 64pt time rail을 항상 예약하고 assistant body는 최대 640pt measure에서 Inter 13pt, 19.5pt line spacing으로 렌더링한다. assistant body는 available width에 맞춰 줄바꿈한다. | 운영자 결정 변경과 승인된 handoff의 token proposal, `MarkdownPreview`의 native selectable rendering을 따른다. |
| D-16 | prompt band는 panel background, mono 12pt, glyph column 16pt, vertical inset 8pt, turn inset 12pt를 사용한다. | 승인된 handoff의 visual contract와 `HideTheme`/design canvas token baseline을 따른다. |
| D-17 | Empty는 session file이 아직 쓰이지 않은 상태로서 `No messages yet`와 `Show terminal` 경로를 보인다. Failed는 file이 존재하지만 읽을 수 없는 상태로서 path와 line을 보이고 `Retry`를 제공하며 terminal을 바꾸지 않는다. | 승인된 handoff 결정 11과 engineering 원칙 4, 10의 caller-visible failure 요구를 따른다. |
| D-18 | non-agent pane이나 지원하지 않는 session source에는 Conversation toggle을 노출하지 않는다. | 승인된 handoff 결정 11과 지원하지 않는 상태를 조용히 추측하지 않는 원칙을 따른다. |
| D-19 | 새 turn은 live append하고 사용자가 bottom에 있을 때만 자동으로 따라간다. 사용자가 위로 scroll하면 follow를 멈추며 새 output이 기존 읽기 위치를 밀어내지 않는다. | 승인된 handoff 결정 7과 기존 native scroll interaction을 따른다. |
| D-20 | Viewer의 assistant body는 native selectable text이며 기존 `MarkdownPreview`의 Markdown parsing, `⌘F` AppKit find, `⌘=`, `⌘-`, `⌘0` text scale을 재사용한다. | 승인된 handoff 결정 7과 engineering 원칙 6, 7에 따른다. |
| D-21 | 모든 새 icon button은 기존 command tooltip modifier와 동일한 accessibility help를 사용한다. | AGENTS.md Design Reference의 shell tooltip 계약과 design 원칙 5, 12를 따른다. |
| D-22 | transcript source에 대한 path, line, pane id 같은 진단만 구조화해 기록한다. prompt/response 본문과 secret은 log에 쓰지 않는다. | 운영자 결정 변경과 engineering 원칙 9, 10을 따른다. |
| D-23 | 새로운 color, radius, spacing, font token은 inline literal로 만들지 않고 먼저 `HideTheme`에 추가한 뒤 `design/hide.pen` generator로 반영한다. | AGENTS.md Design Reference와 design 원칙 5, 8을 따른다. 이번 보드의 off-scale 제안값은 `--proposed-conversation-*`로 기록한다. |
| D-24 | 구현은 `main`에서 분리된 worktree와 PR delivery mode로 수행하고, 구현 branch를 PR로 올리며 CI를 watch한다. merge는 별도 사용자 승인 없이는 수행하지 않는다. | 사용자의 `$please` invocation은 구현과 ship을 승인했지만 merge 승인을 포함하지 않으며 `agents/config.json`은 `delivery.mode: pr`다. |
| D-25 | 이 PRD는 현재 conversation과 운영자 승인 design handoff를 source로 삼으며, 현재 invocation이 implementation authority를 제공한다. `human_approval` frontmatter는 별도 PRD review가 없으므로 pending으로 유지한다. | gen-prd/please 파이프라인의 provenance 규칙과 사용자가 직접 요청한 `$please` 범위를 보존한다. |
| D-26 | 같은 사용자 입력 경계 안의 연속 assistant text/event는 하나의 assistant response block으로 병합한다. 내용 자체는 임의로 요약하거나 삭제하지 않고, tool/thinking/progress와 중복 wrapper만 기존 필터로 제외한다. | 운영자 결정 변경. streaming 조각과 반복 출력으로 인한 verbosity를 줄이면서 원문 답변은 보존한다. |

## Behaviors

| # | 사용자가 관찰하는 행동 | 결정 |
| --- | --- | --- |
| B1 | agent pane의 header에는 terminal과 Conversation 사이를 바꾸는 icon button이 보이고 non-agent pane에는 보이지 않는다. | D-02, D-18 |
| B2 | Conversation을 선택하면 button이 selected 상태가 되고 같은 pane의 terminal은 숨겨지지만 process와 header status는 계속 살아 있다. | D-02, D-06 |
| B3 | 사용자가 `⌘⌥C`를 누르면 현재 eligible pane의 mode가 바뀌고 Settings에서 이 command를 다른 shortcut으로 rebind할 수 있다. | D-03 |
| B4 | 지원되는 agent pane을 열면 기본으로 Conversation Viewer가 보이고, header button 또는 `⌘⌥C`로 terminal과 전환할 수 있다. | D-02, D-03, D-05 |
| B5 | Conversation Viewer는 clock과 elapsed rail을 항상 보여 주며 별도 시간 토글이나 persisted time preference를 제공하지 않는다. | D-04, D-13, D-15, D-22 |
| B6 | Conversation은 위에서 아래로 user prompt band와 사용자 입력 단위로 병합된 assistant response를 번갈아 보여 주며 chat bubble, avatar, tool card를 만들지 않는다. | D-01, D-09, D-10, D-26 |
| B7 | Claude prompt에는 `>`가, Codex prompt에는 `›`가 보이고 나머지 layout과 typography는 같다. | D-10 |
| B8 | user prompt는 panel background 위 mono 12pt로 보이고 assistant text는 bare background 위 selectable Markdown으로 보인다. | D-15, D-16, D-20 |
| B9 | tool call/result, thinking, progress, system reminder, local-command echo가 session에 있어도 viewer에는 표시되지 않는다. | D-09 |
| B10 | assistant Markdown은 기존 heading, paragraph, list, code 등 `MarkdownPreview`가 지원하는 표현과 find/text-scale 동작을 유지한다. | D-20 |
| B11 | 사용자가 body text를 드래그해 선택하고 복사할 수 있으며 `⌘F`, `⌘=`, `⌘-`, `⌘0`가 동작한다. | D-20 |
| B12 | 새 assistant event가 기록되면 현재 사용자 입력에 대응하는 response block에 append되고 사용자가 bottom을 보고 있으면 bottom을 따라가며, 위를 읽는 중이면 현재 위치를 유지한다. 다음 사용자 입력이 오면 새 response block을 시작한다. | D-19, D-26 |
| B13 | 각 turn에 source clock과 elapsed가 보이고 진행 중인 assistant turn은 blue `●`와 현재까지의 elapsed를 보여 준다. | D-13, D-14 |
| B14 | assistant body는 항상 64pt time rail 뒤에서 시작하고 body measure와 available width에 맞춰 줄바꿈한다. | D-15 |
| B15 | session file이 아직 없거나 아직 메시지가 없으면 `No messages yet`와 `Show terminal`을 보여 주고 빈 대화로 성공한 것처럼 꾸미지 않는다. | D-17 |
| B16 | session file이 존재하지만 읽기 실패하면 viewer는 path와 line이 포함된 실패 안내와 `Retry`를 보여 주며 terminal의 내용이나 실행 상태를 바꾸지 않는다. | D-17 |
| B17 | Retry가 성공하면 Failed state가 정상 ledger로 바뀌고, 계속 실패하면 같은 caller-visible error를 유지한다. | D-17, D-22 |
| B18 | agent가 question, approval, error를 요구하는 동안 pane은 notice row가 있는 terminal fallback으로 보이고 transcript를 그 아래에 동시에 보여 주지 않는다. | D-07 |
| B19 | 운영자가 terminal에서 요구를 처리해 demand가 사라지면 pane은 demand 직전의 Conversation/terminal mode로 돌아간다. | D-08 |
| B20 | agent session이 없는 pane, 지원하지 않는 provider, 잘못된 agent session 식별자에는 Conversation button을 노출하지 않고 terminal을 유지한다. | D-18 |
| B21 | narrow pane에서도 body가 available width 안에서 줄바꿈하고 prompt glyph, time rail, action button이 겹치거나 잘리지 않는다. | D-15, D-16, D-21 |
| B22 | header button의 tooltip과 accessibility help가 같은 command 설명을 제공하고, icon의 선택 상태만으로도 현재 mode를 구분할 수 있다. | D-02, D-21 |
| B23 | session read나 parsing이 실패해도 오류는 빈 transcript로 조용히 대체되지 않고 viewer state 또는 retry action으로 드러난다. | D-17, D-22 |
| B24 | transcript refresh 중에도 terminal PTY input과 pane lifecycle은 영향을 받지 않는다. | D-06, D-12 |
| B25 | 지원되는 Claude/Codex JSONL의 operator user와 assistant text가 fixture 및 실제 local session에서 동일한 필터 규칙으로 표시된다. | D-09, D-11 |
| B26 | empty, failed, needs-you, terminal baseline, Claude working, Codex idle 상태가 `design/hide.pen`의 Screen 보드와 같은 구조적 의미를 갖는다. 모든 Conversation 상태는 time rail을 포함한다. | D-01, D-07, D-13, D-17, D-23 |

## Technical structure

이 변경은 macOS SwiftUI shell, `herdr-core`의 core-owned UI state, 그리고 local session reader의 세 경계를 확장한다.

`PaneContent` 또는 이에 준하는 shell presentation model에는 Conversation presentation을 추가하고, pane snapshot/UI snapshot에는 pane mode를 전달한다.

`UiState`에는 pane별 Conversation mode를 저장하며, eligible agent pane의 초기 mode는 Conversation이다. mode 전환은 typed core event로 처리한다.

`PaneShortcutSettings`에는 `toggle_conversation`를 기존 command policy, Settings 표시, rebind 저장 경로에 추가한다. 시간 전용 command는 등록하지 않는다.

기존 `HideIconButton`, pane header layout, `MarkdownPreview`, native `NSTextView` selection/find/text scale 경로를 재사용한다.

provider별 JSONL decoding은 기존 `LocalSessionReader`와 `plugins/agent-context-labels/src/lib.rs`의 parsing 지식을 재사용하되, viewer가 요구하는 timestamp, user-boundary grouping, elapsed, Empty/Failed 결과를 명확히 반환하는 read-only boundary로 정리한다.

reader는 `agent_session`으로 안전하게 허용된 local path만 해석하고, 파일 읽기와 parsing은 runtime mutex 및 render path 밖에서 실행한다.

reader는 처음에는 필요한 tail만 읽고 file modification 또는 append를 감지한 경우에만 갱신하며, 부분적으로 쓰이는 마지막 JSONL record는 다음 refresh에서 재시도한다.

unknown하지만 유효한 provider record는 visible turn으로 만들지 않으며, 파일 open/read 실패는 path와 line을 포함한 Failed result로 반환한다.

core snapshot은 transcript 본문이나 파일 I/O 결과를 mutex 안에 보관하지 않고, shell reader가 pane id와 session source를 이용해 비동기적으로 만든 immutable view model을 render한다.

Conversation mode가 Needs You fallback으로 바뀌는 동안 이전 presentation mode는 shell의 pane-scoped transient state로 보존하고, demand가 해제되면 복원한다.

시간 계산은 source event timestamp가 있는 경우에만 표시하고, timestamp가 없으면 숫자나 임의의 현재 시각을 만들어내지 않는다. 시간 rail은 항상 표시하되 값이 없는 항목은 빈 값으로 꾸미지 않는다.

연속 assistant event는 사용자 입력 경계까지 하나의 response block으로 합치고, 마지막 partial JSONL record는 다음 refresh에서 재시도한다. 원문 assistant text는 보존하며 임의 요약은 하지 않는다.

`HideTheme`에 필요한 Conversation token을 추가하고 `scripts/pen-token-map.json`에 mapping 또는 명시적 proposed 이유를 기록한 뒤 `node scripts/gen-pen.mjs`, `node scripts/check-pen.mjs`, `node scripts/check-design-contract.mjs`를 통과시킨다.

검증은 parser/core unit tests, Swift typecheck/build/test, design contract, 그리고 실제 dev 또는 installed bundle 한 개를 정확히 식별한 native screenshot/interaction evidence로 구성한다.

native 확인에서는 기존 operator app이나 사용자가 소유하지 않은 pane을 종료하거나 재시작하지 않고, isolated Herdr server와 candidate PID/window를 분리한다.

구현 branch에는 source, tests, owning design/token references, 그리고 필요하면 현재 규칙 문서의 변경을 함께 커밋한다.

## Risks

- Claude와 Codex의 JSONL schema가 바뀌면 visible message가 누락되거나 turn grouping이 어긋날 수 있다.
  대응은 provider fixture를 고정하고 unknown record와 실제 session tail을 함께 검증하는 것이다.
- session file이 커지는 동안 마지막 line이 완성되지 않으면 refresh 중 JSON parse 오류가 발생할 수 있다.
  대응은 partial tail을 다음 refresh로 미루고, 지속적인 read failure만 Failed state로 노출하는 것이다.
- local path 권한이나 session id가 잘못되면 사용자는 빈 화면으로 오해할 수 있다.
  대응은 Empty와 Failed를 구분하고 Failed에 path와 line, Retry를 보여 주는 것이다.
- 긴 assistant Markdown이나 narrow pane에서 line wrapping과 scroll anchoring이 깨질 수 있다.
  대응은 기존 native `MarkdownPreview`와 실제 한글/영문 fixture를 사용한 native visual review를 수행하는 것이다.
- `⌘⌥C`가 기존 shortcut과 충돌하면 mode 전환이 조용히 실패할 수 있다.
  대응은 `PaneShortcutSettings`의 reserved/dedup policy와 Settings rebind round-trip test를 추가하는 것이다.
- 여러 assistant event가 하나의 답변으로 합쳐지지 않으면 viewer가 streaming 조각과 중복 wrapper를 반복해 너무 verbose해질 수 있다.
  대응은 사용자 입력 경계 기반 grouping test와 실제 긴 session native review를 수행하는 것이다.
- Needs You fallback이 transcript와 동시에 보이면 운영자가 어떤 surface에 답해야 하는지 혼동할 수 있다.
  대응은 D-07을 whole-pane terminal fallback으로 구현하고 native review에서 notice와 PTY input을 확인하는 것이다.
- transcript 본문이 structured log나 오류 출력에 섞이면 민감한 prompt와 secret이 유출될 수 있다.
  대응은 진단에 path, line, pane id, provider만 허용하고 message body를 기록하지 않는 것이다.
- 구현 중 디자인 token이 inline literal로 추가되면 shell과 canvas가 drift할 수 있다.
  대응은 `HideTheme`를 source of truth로 삼고 design checks를 ship 전 gate로 실행하는 것이다.
- 실제 설치 bundle과 dev build를 혼동하면 native 검증이 잘못된 binary를 증명할 수 있다.
  대응은 candidate PID, bundle path, build identity를 확인하고 정확히 한 instance의 fresh screenshot을 남기는 것이다.
- 기존 dirty worktree나 operator pane을 건드리면 병렬 작업이 손실될 수 있다.
  대응은 별도 worktree, `--no-focus`, read-only inspection을 사용하고 사용자 소유 surface는 건드리지 않는 것이다.
