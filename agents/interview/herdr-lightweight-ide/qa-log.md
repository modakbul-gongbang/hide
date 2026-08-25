---
topic: "herdr 경량 IDE pane: 단축키로 현재 세션 cwd의 파일트리/뷰어 열기"
status: "complete"
where: "greenfield"
selected_packs: "ux, compatibility, verification, operation"
created_at: "2026-08-17"
updated_at: "2026-08-17"
question_count: 17
normalization_policy: "raw-capture-with-checkpoint-backfill"
normalization_checkpoint_every: 10
---

# Interview Log: herdr 경량 IDE pane: 단축키로 현재 세션 cwd의 파일트리/뷰어 열기

## Current Understanding

- 제품: herdr(오픈소스 터미널 멀티플렉서)를 중심에 둔 macOS 네이티브 데스크톱 IDE "herdr-ide". 터미널이 1등 시민이고 파일 워크벤치는 보조.
- 구조: Swift 앱 + libghostty 서피스에 herdr TUI를 통째로 실행(TUI sidebar 숨김) + WKWebView(React)로 native sidebar(spaces/agents)와 오른쪽 파일 워크벤치. herdr 연동은 소켓/CLI(동일 표면).
- 핵심 UX: 선택된 pane에서 단축키 → 오른쪽에 그 pane cwd의 파일트리+뷰어 워크벤치가 열리고 터미널은 유지·축소. 같은 키로 복귀.
- 파일 워크벤치 깊이: 뷰어(이미지/마크다운/코드/diff) + 가벼운 편집(수정·저장) + 검색. LSP/git UI 비목표.
- 리모트: v1부터 리모트 herdr 서버 attach + 리모트 파일 보기 전용. 리모트 편집·검색 비목표.
- 최상위 리스크: libghostty 임베딩 API 비안정 → Ghostty 버전 고정 + 임베딩 스파이크 최우선 선행.
- 원 동기: TUI에서 생성 이미지/아티팩트 확인이 불편, VS Code 새로 열기 번거로움.
- native sidebar v1: 현 TUI sidebar 동등(spaces 전환, agents 상태·포커스, 로컬/리모트 스위처, 새 워크스페이스).
- 워크벤치는 뷰어 확장 구조(파일타입→뷰어 플러그형, 내장 뷰어도 그 위에). 서드파티 배포 생태계는 비목표.
- 단축키 기본 ⌘E(변경 가능). v1 완료 = 골든 시나리오(D-19) 실사용 통과. 앱은 herdr 서버 비독점(기존 터미널 병행 사용 보장).

## Intake Cursor

- next_decision_id: D-35
- next_question: 없음 — 마감
- last_materiality_sweep: checkpoint 5
- outstanding_raw_entries: none
- next_checkpoint_at: Q27

## Decision Register

| ID | Kind | Area | Decision / fact | Priority | Source / owner | Status | PRD mapping / revisit |
| --- | --- | --- | --- | --- | --- | --- | --- |
| D-01 | fact | compatibility | herdr-file-viewer v1.15.0(서드파티, read-only git-aware TUI 뷰어)이 이미 설치되어 prefix+f/prefix+shift+f에 바인딩됨; 이미지 렌더링 미지원 | P0 | repo ~/.config/herdr/config.toml, plugin docs | resolved | D-21로 공존 확정 |
| D-02 | fact | compatibility | herdr experimental.kitty_graphics=true; TUI pane에서 kitty graphics 프로토콜 인라인 이미지 렌더링 가능 | P1 | repo ~/.config/herdr/config.toml | resolved | R#: 이미지 뷰 구현 경로 |
| D-03 | fact | compatibility | (폐기) 초기 preflight에서 herdr 본체를 소스 미보유 바이너리로 판단했으나 D-09(오픈소스 확인)로 대체됨 | P2 | preflight, D-09로 무효화 | rejected | D-09 참조 |
| D-04 | decision | UX/design | 방향: herdr 안의 pane이 아니라, herdr(터미널 멀티플렉서)를 중심에 둔 독립 데스크톱 IDE 앱을 만든다. 터미널 엔진으로 Ghostty를 상정, 그 위에 VSCode 유사 또는 초경량 에디터 껍데기(파일트리/뷰어/에디터)를 씌우는 '나만의 herdr 기반 IDE' | P0 | user, Q1 | resolved | R1: 제품 정체성 |
| D-05 | decision | architecture | 전략 확정: 자체 데스크톱 앱(B). 터미널 엔진 libghostty로 herdr TUI 경험을 그대로 유지하고, 그 위에 IDE 셸을 조립. Tide(Electron+Node+React) 레이아웃을 구조 참고 | P0 | user, Q3 | resolved | R1: 아키텍처 |
| D-06 | decision | UX/design | herdr TUI의 왼쪽 sidebar(spaces, agents)는 앱의 built-in native UI로 분리해 그린다 | P0 | user, Q3 | resolved | R2: 네이티브 사이드바 |
| D-07 | decision | UX/design | 선택된 pane 기준 전환 액션: 누르면 그 pane의 cwd로 VS Code식 file tree/뷰가 열리고, 해당 터미널은 유지된다 | P0 | user, Q3 | resolved | R3: pane→파일트리 전환 |
| D-08 | decision | architecture | 리모트 최종 범위: v1에 리모트 herdr 서버 attach + 리모트 파일 '보기 전용'(트리 탐색, 이미지/코드/마크다운 뷰). 리모트 편집·검색은 비목표(로컬 전용) | P0 | user, Q9 | resolved | R4: 리모트 attach+read-only 파일 뷰; 비목표: 리모트 편집/검색 |
| D-09 | fact | architecture | herdr는 오픈소스(Apache 2.0, Rust, github.com/herdrdev/herdr). server/client 구조이며 CLI와 소켓 API가 동일 표면. SSH bridge로 리모트 지원. 커스텀 GUI 클라이언트가 서버에 직접 attach 가능하고 필요시 본체 수정도 가능 | P0 | https://herdr.dev/ (user 제보, WebFetch 확인) | resolved | R1: 아키텍처 기반 |
| D-10 | decision | UX/design | 파일 영역 깊이: 뷰어(이미지/마크다운/코드 하이라이팅/diff) + 가벼운 편집(단순 수정·저장) + 검색(파일명/텍스트 검색). LSP, git 커밋 UI는 비목표 | P0 | user, Q4 | resolved | R5: 파일 워크벤치 범위; 비목표: LSP/git UI |
| D-11 | decision | architecture | 플러그인 전략: 기존 herdr TUI 플러그인은 터미널 영역 안에서 그대로 동작(공짜 호환). 워크벤치는 뷰어 확장 구조로 설계 — 파일타입→뷰어 매핑이 플러그형이며 내장 뷰어(이미지/마크다운/코드/diff)도 그 구조 위에 구현. 커스텀 뷰어 추가가 핵심 가치. 서드파티 배포 생태계(마켓플레이스)는 v1 비목표 | P0 | user, Q12 | resolved | R6: 뷰어 확장 구조; 비목표: 배포 생태계 |
| D-12 | decision | UX/design | 전환 레이아웃: 단축키로 터미널 오른쪽에 파일트리+뷰어 워크벤치 패널이 열리고(터미널 유지·축소), 같은 키로 닫아 원상복구. 트리 루트는 선택된 pane의 cwd | P0 | user, Q5 (추천 수용) | resolved | R3/AC: 전환 UX |
| D-13 | decision | architecture | 앱 스택 최종: macOS 네이티브 Swift 앱 + libghostty 터미널 서피스 + WKWebView(React) 사이드바/워크벤치. herdr 연동은 소켓/CLI. macOS 전용(v1) | P0 | user, Q10 (리서치 후 추천 수용) | resolved | R7: 스택 확정; 비목표: 크로스플랫폼 v1 |
| D-14 | decision | risk | libghostty 비안정 대응: Ghostty 버전 고정 + 임베딩 스파이크를 최우선 태스크로 선행, 실패/파손 시 재논의 | P1 | user, Q10 확인 | resolved | 리스크 대응; revisit: libghostty 안정 릴리스 |
| D-15 | fact | architecture | 리서치: libghostty 풀 임베딩 API는 비안정(유일 소비자 Ghostty macOS Swift 앱), Tauri×libghostty 선례 전무, Tauri 터미널은 전부 xterm.js+pty. 안정화 근접한 건 libghostty-vt(파서)뿐 | P0 | ghostty 공식 docs, mitchellh.com, GitHub (2026-08 WebSearch) | resolved | R7 근거 |
| D-16 | decision | architecture | 터미널 영역: libghostty 서피스 하나에 herdr TUI 클라이언트를 통째로 실행(기존 인터페이스/단축키 100% 유지). TUI sidebar는 숨김 — herdr에 숨김 옵션이 없으면 본체에 추가. native sidebar 조작은 herdr CLI/소켓 명령으로 전달 | P0 | user, Q10 (추천 수용) | resolved | R8: 터미널 통합 방식 |
| D-17 | decision | UX/design | native sidebar v1 범위: 현 TUI sidebar 기능 동등 — spaces 목록/전환, agents 목록(상태·attention 뱃지, 클릭 포커스), 로컬/리모트 서버 스위처, 새 워크스페이스 생성. 추가 기능 없음 | P0 | user, Q11 (추천 수용) | resolved | R2/AC: 사이드바 범위 |
| D-18 | decision | UX/design | 워크벤치 전환 전역 단축키 기본값 ⌘E, 설정에서 변경 가능 | P2 | user, Q13 (추천 수용) | resolved | AC: 단축키 |
| D-19 | decision | verification | v1 골든 시나리오: herdr-ide로 하루 작업 시작 → 에이전트 생성 이미지를 해당 pane에서 ⌘E로 3초 내 확인 → 가벼운 수정·저장 → mini 리모트 herdr를 사이드바에서 attach해 상태 확인. 이 시나리오 실사용 통과가 v1 완료 기준 | P1 | user, Q14 | resolved | V#: 골든 시나리오 |
| D-20 | decision | operation | 병행 사용 보장: 앱은 herdr 서버를 독점하지 않으며, 기존 터미널에서 herdr CLI/client로 같은 서버에 붙는 사용이 계속 가능해야 한다 | P1 | user, Q14 | resolved | R9/AC: 서버 공존 |
| D-21 | decision | compatibility | 기존 herdr-file-viewer 플러그인은 그대로 공존: TUI 안에서 prefix+f로 계속 동작. ⌘E(앱 레벨)와 prefix+f(TUI 레벨)는 키 체계가 달라 충돌 없음 | P0 | user, Q15 | resolved | R#: 기존 뷰어 공존 (D-01 해소) |
| D-22 | decision | UX/design | 워크벤치 포커스 정책: ⌘E 누른 시점의 pane cwd에 고정. 다른 pane 기준으로 보려면 그 pane에서 다시 ⌘E. 재오픈 시 상태 복원은 최소(트리 루트 재계산)로 시작 | P1 | user, Q15 | resolved | R3/AC: 포커스 정책 |
| D-23 | decision | data | 동시 수정 충돌: 외부 변경 감지 시 단순 경고 + 재로드 선택지 제공(미저장 변경 보존형 diff/병합은 비목표) | P1 | user, Q15 | resolved | R5/AC: 충돌 처리; 비목표: diff 병합 |
| D-24 | decision | operation | 엔지니어링 방침: ① herdr 버전 고정(앱은 검증된 버전 대상, 업데이트는 수동 검증 후) ② herdr sidebar 숨김 미지원 시 포크+upstream PR, 머지까지 포크 유지 ③ 리모트 인증은 herdr 기존 SSH bridge(~/.ssh/config) 재사용, 앱 자체 자격증명 저장 없음 ④ libghostty 스파이크 성공 기준 = 서피스에서 herdr TUI 입력/리사이즈/스크롤/kitty graphics 정상 동작 | P1 | user, Q15 일괄 승인 | resolved | R#: 버전 고정/포크 전략/인증/스파이크 기준 |
| D-25 | assumption | UX/design | 워크벤치 재오픈 시 상태 복원 없음(트리 루트 재계산만). 마지막 열람 파일/스크롤 복원은 v1 비목표 | P2 | agent 기본값 (D-22 연장) | resolved | AC: 재오픈 동작. revisit: 실사용 불편 시 |
| D-26 | assumption | operation | herdr 포크 리베이스 트리거: 고정 herdr 버전을 올리는 시점에 포크도 함께 리베이스·재검증 | P2 | agent 기본값 (D-24② 연장) | resolved | 운영: 포크 유지 규칙 |
| D-27 | decision | architecture | 뷰어 확장 v1 깊이: 내부 확장 구조만 — 뷰어 인터페이스는 코드 내부 계약(파일타입→뷰어 컴포넌트 등록), 새 뷰어는 herdr-ide 코드에 직접 추가. 외부 플러그인 계약(매니페스트/샌드박싱/설치)은 후속 비목표 | P1 | user, Q16 | resolved | R6 구체화; 비목표: 외부 뷰어 플러그인 계약 |
| D-28 | assumption | operation | 포크 빌드/배포: 로컬 빌드로 개인 사용, 서명/자동배포 없음. 기존 herdr 설정·플러그인 호환은 병행 사용 스모크(D-20 검증)로 확인 | P2 | agent 기본값 | resolved | 운영. revisit: 배포 필요 시 |
| D-29 | assumption | verification | 골든 시나리오 '3초 내 확인'은 수동 측정(스톱워치/체감)으로 판정하는 실사용 기준이며 자동화 계측은 비목표 | P2 | agent 기본값 | resolved | V# 측정 방법 |
| D-30 | decision | UX/design | cwd 폴백 최종: pane cwd도 마지막 알던 경로도 없으면 홈 디렉토리를 루트로 열고 'pane 경로를 찾지 못함' 상태 표시 | P1 | user, Q17 | resolved | UX-01 recovery 확정 |
| D-31 | decision | UX/design | 리모트 인증 실패 화면: 오류 사유 + 재시도 버튼 + '~/.ssh/config의 해당 호스트 설정 확인' 안내 문구 | P1 | user, Q17 | resolved | UX-02 failure 확정 |
| D-32 | decision | data | diff 뷰어 기준: git 리포면 작업트리 vs HEAD(변경 전체), git 리포가 아니면 diff 뷰어 비활성 | P1 | user, Q18 | resolved | R5: diff 기준 |
| D-33 | decision | data | 검색 범위: .gitignore 존중 + 숨김파일 기본 제외(토글 가능), 사전 인덱싱 없이 ripgrep식 온디맨드 검색 | P1 | user, Q18 | resolved | R5: 검색 범위 |
| D-34 | decision | risk | herdr 포크 장기 리스크: sidebar 숨김은 소규모 패치로 upstream 미병합 시에도 포크 유지 비용이 작다고 보고 수용 리스크로 확정 | P1 | user, Q18 | resolved | 리스크 수용. revisit: 패치 규모 증가 시 |

## Raw Q&A

### Q1: 제품 형태
- decision_ids: D-04
- route: user-decision
- asked: 경량 IDE 구현 형태 방향? (TUI pane / GUI 소환 / Browser pane / 하이브리드)
- recommended: TUI pane + kitty 이미지
- answer: VSCode같은 IDE를 껍데기로만 쓰고 그 안에서 herdr 터미널을 그대로. Ghostty가 엔진, 껍데기는 vscode나 엄청 가벼운 파일 에디터. herdr 베이스로 나만의 IDE를 만드는 느낌
- immediate_notes: 제시한 4개 옵션 전부 기각. pane 통합이 아니라 역방향: 터미널(herdr)이 1등 시민인 데스크톱 IDE. 셸 기술 스택은 미정
- needs_normalization: false

### Q2: B 확정+레이아웃
- decision_ids: D-05, D-06, D-07, D-08
- route: user-decision
- asked: 최종 전략 선택 (VS Code 껍데기 vs 자체 앱)
- recommended: VS Code 껍데기
- answer: B로 가도 괜찮을듯. 지금 인터페이스 유지 + 로컬/리모트 추가. 선택된 pane에서 전환하면 그 경로의 file tree가 열리고 터미널도 유지. Tide 구조 참고. spaces/agents는 built-in native로
- immediate_notes: decision: 자체 앱 확정. constraints: 기존 herdr 사용 경험 보존이 1순위. reference: Tide 스크린샷
- needs_normalization: false

### Q3: herdr 소스 접근
- decision_ids: D-09
- route: mixed
- asked: herdr 본체 소스 접근/수정 가능 여부
- recommended: none
- answer: 오픈소스일껄, CLI도 있고. herdr.dev 확인해봐
- immediate_notes: herdr.dev 확인 결과 오픈소스 확정. native sidebar는 소켓 API 구독으로 구현 가능, IDE 앱은 herdr server의 또 다른 클라이언트가 되는 구조가 정석
- needs_normalization: false

### Q4: 편집 깊이
- decision_ids: D-10, D-11
- route: user-decision
- asked: 파일 영역 깊이: 뷰어 전용 / 뷰어+가벼운 편집 / 진짜 에디터
- recommended: 뷰어+가벼운 편집
- answer: 2번 + 검색 정도까지만. 최대한 플러그인들 있으면 붙일 수 있으면 좋겠다
- immediate_notes: 검색이 추가 스코프로 들어옴. 플러그인 활용은 희망사항 수준으로 개방
- needs_normalization: false

### Q5: 전환 레이아웃
- decision_ids: D-12
- route: user-decision
- asked: 전환 UX 배치: 오른쪽 워크벤치 / 탭 토글 / 에디터 중앙+터미널 하단
- recommended: 오른쪽 워크벤치 패널
- answer: 1번 그래 ㄱㄱ
- immediate_notes: 명확한 직전 추천 수용
- needs_normalization: false

### Q6: 리모트 범위
- decision_ids: D-08
- route: user-decision
- asked: 리모트 범위: 로컬만+구조 개방 / 리모트 attach 포함 / 리모트 파일까지
- recommended: v1 로컬만, 리모트는 구조만 개방
- answer: v1부터 1(리모트 herdr 서버 attach)까지 포함
- immediate_notes: 리모트 pane에서 파일트리 전환 시의 동작은 후속 확인 필요할 수 있음 — 파일 접근 비목표라 비활성 또는 안내 처리
- needs_normalization: false

### Q7: 스택+리모트 확장
- decision_ids: D-13, D-14, D-08
- route: user-decision
- asked: Q7 리모트 pane 전환 동작 / Q8 앱 기술 스택
- recommended: Q7: 미지원 안내, Q8: Swift 네이티브
- answer: Q8은 2번(Tauri) 해보면 좋겠고, Q7은 리모트 파일 보기도 연결해주면 좋겠다
- immediate_notes: 리모트 파일 보기가 v1 스코프로 들어와 D-08 재오픈. Tauri는 추천과 다른 선택 — 스파이크 리스크 수용 여부와 fallback은 별도 확인
- needs_normalization: false

### Q8: 리모트 보기전용
- decision_ids: D-08
- route: user-decision
- asked: 리모트 파일 깊이: 보기 전용 vs 로컬과 동등
- recommended: 보기 전용
- answer: 우선 a(보기 전용). Tauri libghostty 서피스 있는지 리서치 요청
- immediate_notes: Q10(fallback)은 리서치 결과 본 뒤 답하기로 유보
- needs_normalization: false

### Q9: 스택 최종
- decision_ids: D-15, D-13, D-14
- route: user-decision
- asked: 리서치 결과 기반 스택 재선택 (Tauri+xterm.js / Tauri+libghostty 강행 / Swift 네이티브)
- recommended: Swift 네이티브 + libghostty
- answer: 우선 3번으로 ㄱㄱㄱ
- immediate_notes: Tauri 선택(구 D-13)을 리서치 근거로 번복, Swift 확정
- needs_normalization: false

### Q10: 터미널 통합
- decision_ids: D-16, D-14
- route: user-decision
- asked: 터미널 영역: herdr TUI 통째 vs pane 단위 재구성 + 스파이크 방침 확인
- recommended: TUI 통째 + 스파이크 선행
- answer: a로 하고 스파이크 방침도 ㅇㅋ
- immediate_notes: herdr sidebar 숨김 옵션 존재 여부는 구현 시 확인, 없으면 upstream 수정
- needs_normalization: false

### Q11: 사이드바 범위
- decision_ids: D-17
- route: user-decision
- asked: native sidebar v1 기능 범위
- recommended: TUI sidebar 기능 동등 + 서버 스위처
- answer: 음 우선 그정도
- immediate_notes: 확장(요약/알림 히스토리 등)은 후속
- needs_normalization: false

### Q12: 뷰어확장+단축키
- decision_ids: D-11, D-18
- route: user-decision
- asked: Q12 IDE 플러그인 시스템 비목표 제안 / Q13 전역 단축키 기본값
- recommended: Q12: 비목표, Q13: ⌘E
- answer: Q12: 커스텀 뷰어 꽂는 게 핵심 아니냐(비목표 반대). Q13: 우선 ㅇㅇ
- immediate_notes: 추천 기각→뷰어 확장 구조를 1급 요구로 승격. 서드파티 배포 생태계까지는 요구 아님으로 해석(확장 포인트만)
- needs_normalization: false

### Q13: 성공기준+공존
- decision_ids: D-19, D-20
- route: user-decision
- asked: v1 골든 시나리오와 서버 병행 사용 보장 확인
- recommended: 제시안 그대로
- answer: ㅇㅇㅇ 훌륭하다
- immediate_notes: 명확한 직전 제안 수용
- needs_normalization: false

### Q14: 게이트 지적 해소
- decision_ids: D-21, D-22, D-23, D-24
- route: user-decision
- asked: gap-audit 지적: 기존 뷰어 관계 / 포커스 추적 / 충돌 처리 / 방침 4건
- recommended: 공존, 고정, 경고+diff, 4건 승인
- answer: 공존, 연 시점 고정, 단순 경고+재로드(추천보다 단순안 선택), 4건 모두 승인
- immediate_notes: 충돌 처리는 추천(diff 후 선택)이 아닌 단순안 채택 — diff/병합은 명시적 비목표
- needs_normalization: false

### Q15: 뷰어 확장 깊이
- decision_ids: D-27, D-28, D-29
- route: user-decision
- asked: 뷰어 확장 v1 깊이: 내부 구조만 vs 외부 플러그인 계약
- recommended: 내부 확장 구조만
- answer: ㅇㅇㅇ a로
- immediate_notes: P2 두 건은 가역적 내부 기본값으로 함께 등록
- needs_normalization: false

### Q16: 폴백 UX 확정
- decision_ids: D-30, D-31
- route: user-decision
- asked: cwd 최종 폴백 / 리모트 인증 실패 화면 액션
- recommended: 홈 디렉토리+상태표시 / 재시도+SSH config 안내
- answer: ㅇㅇㅇㅇ (추천 수용)
- immediate_notes: 명확한 직전 추천 수용
- needs_normalization: false

### Q17: 게이트 잔여 P1 해소
- decision_ids: D-32, D-33, D-34
- route: user-decision
- asked: diff 기준 / 검색 범위 / 포크 리스크 / D-20 proof
- recommended: 작업트리 vs HEAD, gitignore+ripgrep, 수용 리스크, 병행 attach proof
- answer: ㅇㅇㅇ (추천 일괄 수용, 게이트 재실행 지시)
- immediate_notes: D-20 proof는 UX 카드에 추가
- needs_normalization: false

## UX Scenario Cards

### UX-01: pane → 파일 워크벤치 전환
- trigger: herdr TUI에서 특정 pane이 선택된 상태로 앱 전역 단축키를 누른다
- happy path: 터미널 오른쪽에 워크벤치 패널이 열리고(터미널 유지·축소), 그 pane의 cwd를 루트로 한 파일트리가 보인다. 이미지 파일 클릭 → 즉시 이미지 뷰, 마크다운 → 렌더링 뷰, 코드 → 하이라이팅 뷰. 같은 키로 닫으면 터미널이 원래 폭으로 복귀
- state / failure: 선택된 pane의 cwd를 herdr에서 얻지 못한 경우(죽은 pane 등), 리모트 pane인 경우(보기 전용으로 열림, 편집·검색 비활성)
- recovery: cwd 획득 실패 시 마지막으로 알던 경로, 그것도 없으면 홈 디렉토리를 루트로 열고 'pane 경로를 찾지 못함' 상태 표시 (D-30)
- proof: 실제 앱에서 pane 선택 → 단축키 → 해당 cwd 트리 표시와 이미지 렌더링을 스크린샷으로 확인
- linked decisions: D-07, D-12, D-08, D-10

### UX-02: native sidebar에서 herdr 조작
- trigger: 앱 좌측 native sidebar에서 space 또는 agent 항목 클릭
- happy path: herdr 소켓/CLI로 전환 명령이 전달되어 터미널 영역의 herdr TUI가 해당 워크스페이스/agent pane으로 전환. agent 상태(working/attention 등)가 사이드바에 실시간 반영
- state / failure: herdr 서버 연결 끊김(사이드바가 연결 상태를 표시하고 재연결 시도), 리모트 서버 인증 실패
- recovery: 재연결 버튼/자동 재시도. 리모트 인증 실패 시 오류 사유 + 재시도 버튼 + '~/.ssh/config 해당 호스트 설정 확인' 안내 (D-31)
- proof: 사이드바 클릭이 TUI 전환으로 이어지는 흐름과 상태 갱신을 실사용으로 확인
- linked decisions: D-06, D-08, D-09, D-16

### UX-03: 가벼운 편집과 저장
- trigger: 워크벤치에서 로컬 텍스트 파일을 열고 수정
- happy path: 인라인 편집 → 저장 단축키 → 디스크 반영, 트리/뷰 갱신
- state / failure: 외부(에이전트)에서 같은 파일이 동시에 변경된 경우 — 변경 감지 시 단순 경고 표시(diff/병합 비목표)
- recovery: 디스크 버전 다시 불러오기 선택지 제공
- proof: 편집·저장 후 터미널에서 cat으로 동일 내용 확인, 외부 변경 경고 시나리오 재현
- linked decisions: D-10, D-23

### UX-04: 파일/텍스트 검색
- trigger: 워크벤치 열린 상태에서 검색 입력(파일명 필터 또는 텍스트 검색)
- happy path: cwd 하위에서 결과 목록 표시, 항목 클릭 시 해당 파일이 뷰어로 열림(텍스트 검색은 해당 위치로 이동)
- state / failure: 결과 없음(빈 상태 문구 표시), 대규모 트리에서 검색 지연(진행 표시), 리모트 pane에서는 검색 비활성(D-08)
- recovery: Esc로 검색 취소 후 트리로 복귀
- proof: 알려진 파일명/문자열 검색이 결과와 열람으로 이어지는 흐름 확인
- linked decisions: D-10, D-08, D-33

### UX-05: herdr 서버 병행 사용
- trigger: herdr-ide 앱이 실행 중인 상태에서 별도 터미널에서 herdr CLI/client 사용
- happy path: 별도 터미널의 herdr attach와 CLI 명령이 정상 동작하고, 양쪽의 상태 변화가 서로 반영됨
- state / failure: 앱이 서버를 독점해 외부 attach가 실패하는 경우(허용 안 됨)
- recovery: 해당 없음(보장 요건)
- proof: 앱 실행 중 별도 터미널에서 herdr attach 성공 + 워크스페이스 전환이 앱 사이드바에 반영되는지 확인
- linked decisions: D-20

## Evidence From Code, Docs, Or Research

- herdr 오픈소스: https://herdr.dev/ , github.com/herdrdev/herdr (Apache 2.0, Rust, CLI=소켓 API 동일 표면, SSH bridge)
- 기존 설치 환경: ~/.config/herdr/config.toml (prefix+f → herdr-file-viewer, experimental.kitty_graphics=true)
- libghostty 상태: https://ghostty-org-ghostty.mintlify.app/api/overview , https://mitchellh.com/writing/libghostty-is-coming — 풀 임베딩 API 비안정, 유일 검증 소비자는 Ghostty macOS Swift 앱. Tauri 선례 없음(2026-08 WebSearch)
- 레이아웃 참고: Tide (github.com/team-attention/tide, Electron+Node+React) 스크린샷 — 좌측 native 사이드바 + 중앙 작업영역 + 우측 워크벤치/파일트리 구조

## Documented Domain Checks

- docs inspected: herdr.dev, ghostty libghostty API docs, herdr plugin manifests(~/.config/herdr/plugins), ~/.config/herdr/config.toml
- canonical terms: space/workspace, agent pane, attention, prefix key, plugin action, workbench(본 프로젝트 신조어: 우측 파일 패널)
- glossary or code conflicts: 초기 "herdr 바이너리 소스 미보유" 판단이 오픈소스 확인으로 폐기(D-03→D-09)
- concrete scenarios tested: 없음(설계 인터뷰 단계)
- docs mutation: 없음
- ADR candidate: 스택 선택(Swift+libghostty, Tauri 기각 근거 포함), herdr TUI 통째 실행 vs pane 재구성

## Checkpoint And Sweep History

### Checkpoint 1
- after_question: Q10
- normalized_entries: Q1, Q2, Q3, Q4, Q5, Q6, Q7, Q8, Q9, Q10
- register_changes: D-04~D-16 확립: 자체 Swift 앱 + libghostty + herdr TUI 통째 실행 + native sidebar + 우측 워크벤치, 리모트 attach+보기전용, 편집은 라이트+검색, 스파이크 방침. D-13은 Tauri→Swift로 번복 이력 있음
- reopened_decisions: none
- highest_remaining_gap: native sidebar 기능 범위와 단축키 체계, 플러그인 전략(D-11), 검증/운영 미논의

### Checkpoint 2
- after_question: Q13
- normalized_entries: Q11, Q12, Q13
- register_changes: D-17~D-20 추가(사이드바 범위, 뷰어 확장 구조 승격, ⌘E, 골든 시나리오, 서버 공존), D-03 폐기(D-09로 대체)
- reopened_decisions: none
- highest_remaining_gap: 없음 — 남은 세부는 P2 구현 재량

### Checkpoint 3
- after_question: Q14
- normalized_entries: Q14
- register_changes: D-21~D-24 추가(공존, 포커스 고정, 충돌 단순 경고, 방침 4건), D-01 매핑 갱신, UX-03 수정, UX-04 검색 카드 추가
- reopened_decisions: none
- highest_remaining_gap: 없음

### Checkpoint 4
- after_question: Q16
- normalized_entries: Q15, Q16
- register_changes: D-27~D-31 추가(뷰어 확장 내부 구조 한정, 포크 빌드/3초 측정 기본값, cwd 폴백, 인증 실패 UX). UX-01/02 recovery 갱신
- reopened_decisions: none
- highest_remaining_gap: 없음

### Checkpoint 5
- after_question: Q17
- normalized_entries: Q17
- register_changes: D-32~D-34 추가(diff 기준, 검색 범위, 포크 수용 리스크), UX-05 병행 사용 카드 추가
- reopened_decisions: none
- highest_remaining_gap: 없음

## Audit History

### Audit 1
- type: gap-audit-gate
- result: pass
- missing decision_ids: 없음
- unsupported assumptions: 없음
- UX or behavior gap: 없음 (P2 잔여 1건: upstream PR 거부 시 재논의 트리거 미명시 — 수용)
- highest-risk blocker: 없음
- final-blocking-question: 없음
- PRD impact: D-01~D-34 전체가 PRD 소스로 사용 가능
