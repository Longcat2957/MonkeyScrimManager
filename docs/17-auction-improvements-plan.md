# Auction Workflow Improvements Implementation Plan

> **For Hermes:** Use Codex only for implementation. Do not use Claude Code. Implement task-by-task with TDD/focused verification.

**Goal:** 애니메이션 미동작 운영 버그를 최우선으로 해결한 뒤, 반영 요청된 경매/내전 운영 개선사항 5개를 최소 범위로 안정적으로 구현한다.

**Architecture:** 최우선 작업은 FAB → 미니게임 → 미니게임전체 애니메이션이 dev 외 환경에서 결과만 끊겨 보이는 root cause를 재현/격리/수정하는 것이다. 이후 변경은 대부분 기존 경매/일반내전 웹 플로우와 API command contract 조정이다. DB에 직접 컬럼을 추가하지 않는 요구사항은 API payload/notification/embed metadata 또는 표시용 snapshot 필드 수준으로 처리하고, 기존 dirty `apps/web/**` 변경과 충돌하지 않게 작업 전 diff를 분리 확인한다.

**Tech Stack:** Svelte 5, TypeScript, bits-ui Dialog, Rust axum API, sqlx/PostgreSQL, Vitest, cargo test/check.

---

## 요구사항 정리

0. **최우선:** FAB → 미니게임 → 미니게임전체 애니메이션이 dev 환경 외에서 동작하지 않고 결과만 끊겨 보이는 문제를 해결한다.
   - dev 환경에서는 정상 동작.
   - VPS 성능 문제는 아님이 확인됨.
   - prod/preview/build 환경 차이, CSS/transition bundling, reduced-motion 처리, dynamic import/hydration, timer/requestAnimationFrame 경로를 우선 조사한다.
1. 경매 점수 전환 이후 timer를 progress bar 감소 형태로 표시한다.
   - 기본값: 10초.
   - 경매 점수를 올리거나/내리는 callback에서 timer 초기화.
   - progress bar 위치는 현재 레이아웃/컴포넌트 위치를 파악한 뒤 협의한다.
2. 내전모집 객체 생성 시 required 입력값을 추가한다.
   - 예상 시작 시간을 입력할 수 있게 한다.
   - DB 컬럼에 직접 추가하지 않는다.
3. 일반내전 → 경매내전 전환을 포지션 선택 없이 바로 동작 가능하게 일부 조정한다.
4. 경매 잔여 인원 표시는 Button Action → Modal display 형식으로 구현한다.
5. 피어리스 설정 기본값을 `always=True`가 되도록 한다.

---

## 사전 확인 결과

주요 후보 파일:

- 경매 입찰 작업대: `apps/web/src/lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte`
  - 현재 bid 입력/증감: `adjustBid(team, delta)`
  - 입찰 확정: `finalizeBid(team)`
  - 잔여 인원 계산값: `remainingCount`, `queuedCandidates`, `unsold`
  - 이미 `bits-ui` Dialog 사용 중
- 경매 모집 생성 화면: `apps/web/src/routes/g/[guildId]/auction/+page.svelte`
  - 현재 `createHardFearless = false`
  - `openCreateDialog()`에서도 false로 reset
  - `createAuctionRecruitment(...)` 호출 payload에 `hardFearless` 포함
- 경매 모집 API client: `apps/web/src/lib/api/auctionRecruitment.ts`
  - `AuctionRecruitmentCreateOptions`
  - client validation
- 일반내전 엔트리 → 경매 전환 화면: `apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/+page.svelte`
  - `createAuctionRecruitment()`가 현재 `hasCompleteAssignments` 조건을 요구
- 전환 API route: `apps/api/src/http/bot_routes.rs`
  - `convert_entry_draft_to_auction_recruitment(...)`
  - 관련 에러: `auction_declared_lane_required`
- backend workflow rules: `apps/api/src/repository/bot_workflow.rs`
  - `RecruitmentSeriesRules::from_optional(... hard_fearless.unwrap_or(false))`
  - `AuctionRecruitmentSeriesRules::from_optional(... hard_fearless.unwrap_or(false))`

---

## 확정: progress bar 위치

사용자 확정안: **작업대 전체 헤더 하단 full-width thin bar**.

구현 기준:

- 경매 입찰 작업대 헤더 바로 아래에 전체 폭 progress bar를 둔다.
- bar는 10초에서 0초로 줄어드는 형태다.
- 점수 `+/-` 조정 callback에서 10초로 reset한다.
- 개별 팀 카드/입찰 버튼 주변에는 별도 timer UI를 추가하지 않는다.
- 타이머는 시각적 운영 보조만 하며, 자동 입찰/확정 side effect는 만들지 않는다.

---

## Phase 0: 작업 안전장치

### Task 0.0: 최우선 애니메이션 미동작 root cause 조사 및 수정

**Objective:** FAB → 미니게임 → 미니게임전체 애니메이션이 dev 외 환경에서 결과만 끊겨 보이는 원인을 먼저 해결한다.

**Known facts:**
- dev 환경에서는 정상 동작한다.
- dev 외 환경에서는 어떤 애니메이션 동작도 보이지 않고 결과만 즉시 표시된다.
- VPS 성능 문제는 아님이 확인됐다.

**Likely files:**
- `apps/web/src/lib/components/fab/QuickActionFab.svelte`
- `apps/web/src/lib/components/fab/MiniGameModal.svelte`
- `apps/web/src/lib/components/fab/TimerModal.svelte`
- `apps/web/src/lib/components/fab/LadderGame.svelte`
- `apps/web/src/lib/components/fab/RouletteGame.svelte`
- `apps/web/src/lib/components/fab/CoinTossGame.svelte`
- `apps/web/vite.config.*`, `apps/web/svelte.config.*`, global CSS if animation classes are purged/minified

**Investigation checklist:**
1. Compare `pnpm dev` vs `pnpm build && pnpm preview` behavior locally.
2. Inspect browser console/runtime state in preview build.
3. Check whether animation code depends on `import.meta.env.DEV`, debug-only delays, `setTimeout` stripped/short-circuited, transition classes missing from production CSS, `prefers-reduced-motion`, SSR/hydration timing, or dynamic import boundaries.
4. Add a regression test or focused component test that fails when animation phase jumps directly to result.
5. Fix root cause only; do not bundle auction feature changes into this task.

**Verification:**
- Local production-equivalent build/preview reproduces before fix or evidence explains why not.
- Focused tests pass.
- Manual/browser verification in preview build confirms animation phases are visible before result.
- `pnpm --filter @monkeybot/web build` passes.

### Task 0.1: 작업 전 상태 분리 확인

**Objective:** 기존 unrelated dirty 변경과 이번 작업 변경을 섞지 않는다.

**Files:** 없음

**Steps:**
1. `git status --short` 확인.
2. 기존 dirty `apps/web/**` 변경 목록을 별도 기록.
3. 이번 작업에서 수정할 파일만 `git diff -- <paths>`로 확인한다.

**Verification:** 기존 dirty 파일을 revert하지 않고 보존한다.

---

## Phase 1: 피어리스 기본값 always=True

### Task 1.1: Backend 기본값 테스트 추가

**Objective:** hard/fearless 옵션이 생략되면 true가 되는 contract를 고정한다.

**Files:**
- Modify/Test: `apps/api/src/repository/bot_workflow.rs`
- Modify/Test: `apps/api/src/http/bot_routes.rs` 또는 기존 request parsing tests

**Steps:**
1. `AuctionRecruitmentSeriesRules::from_optional(..., hard_fearless: None)`이 `true`를 반환하는 테스트 추가.
2. 일반 내전 `RecruitmentSeriesRules::from_optional(..., hard_fearless: None)`도 요구사항 범위에 포함할지 확인 후 동일하게 테스트 추가.
3. 테스트 실패 확인.

**Commands:**
- `cargo test -p monkeybot-api hard_fearless -- --nocapture`

### Task 1.2: Backend 기본값 구현

**Objective:** API/bot command에서 hardFearless 생략 시 true가 되게 한다.

**Files:**
- Modify: `apps/api/src/repository/bot_workflow.rs`
- 필요 시 Modify: `apps/api/src/http/bot_routes.rs`

**Implementation:**
- `hard_fearless.unwrap_or(false)` → `hard_fearless.unwrap_or(true)`
- `Default` 구현의 `hard_fearless: false` → `true`

**Verification:**
- `cargo test -p monkeybot-api hard_fearless`
- `cargo check -p monkeybot-api`

### Task 1.3: Frontend 생성 기본값 변경

**Objective:** 경매 모집 생성 모달 기본 선택도 true로 맞춘다.

**Files:**
- Modify: `apps/web/src/routes/g/[guildId]/auction/+page.svelte`
- Test: `apps/web/src/routes/g/[guildId]/auction/page.test.ts`

**Implementation:**
- `let createHardFearless = $state(false);` → `true`
- `openCreateDialog()` reset도 `true`

**Verification:**
- `pnpm --filter @monkeybot/web test -- apps/web/src/routes/g/[guildId]/auction/page.test.ts`

---

## Phase 2: 경매 모집 생성 required 예상 시작 시간

### Task 2.1: 표현 방식 확정

**Objective:** DB 컬럼 추가 없이 예상 시작 시간을 어디에 보존/전달할지 결정한다.

**Recommended approach:**
- DB 컬럼 직접 추가 없음.
- API command payload에는 `expectedStartTimeText` 또는 `expectedStartAtText` required string 추가.
- 서버는 recruitment title/message/embed 또는 notification payload의 description/metadata에 반영한다.
- 영속적인 검색/정렬 요건이 없으므로 별도 컬럼은 만들지 않는다.

**Open decision:** 기존 DB에 JSON metadata/notes 필드가 있으면 거기 사용. 없으면 Discord/notification 표시용 텍스트로만 처리.

### Task 2.2: Frontend contract/client 테스트 추가

**Objective:** 경매 모집 생성 API client가 예상 시작 시간을 required로 보낸다.

**Files:**
- Modify/Test: `apps/web/src/lib/api/auctionRecruitment.ts`
- Modify/Test: `apps/web/src/lib/api/auctionRecruitment.test.ts`

**Test cases:**
- 빈 문자열이면 client validation error.
- 정상 문자열이면 POST body에 포함.
- 예: `오늘 22:30`, `23:00`, `7/26 21:30` 등 자유 텍스트 허용.

### Task 2.3: 경매 생성 모달 UI 추가

**Objective:** 경매 모집 생성 Dialog에 required 예상 시작 시간 입력을 추가한다.

**Files:**
- Modify: `apps/web/src/routes/g/[guildId]/auction/+page.svelte`
- Test: `apps/web/src/routes/g/[guildId]/auction/page.test.ts`

**Implementation:**
- state: `createExpectedStartTime = $state('')`
- `openCreateDialog()`에서 초기화.
- 생성 버튼 disabled 조건: trim 값 없으면 disabled.
- submit payload에 포함.
- label/help text: `예상 시작 시간`, placeholder `예: 오늘 22:30`

**Verification:**
- UI test: 입력 전 생성 불가, 입력 후 payload 포함.

### Task 2.4: Backend request/validation 반영

**Objective:** 서버가 새 required 값을 검증하고 downstream 표시 정보에 전달한다.

**Files:**
- Modify: `apps/api/src/http/bot_routes.rs`
- Modify: `apps/api/src/repository/bot_workflow.rs`
- Modify: `apps/api/src/repository/notification_outbox.rs` if Discord message/embed에 표시 필요
- Test: 관련 cargo tests

**Validation:**
- trim empty → 400 invalid field
- max length 64 또는 100 chars
- credential/sensitive info와 무관

**Verification:**
- `cargo test -p monkeybot-api auction_recruitment`
- `cargo check -p monkeybot-api`

---

## Phase 3: 일반내전 → 경매내전 전환 시 포지션 선택 없이 동작

### Task 3.1: 현재 blocking 조건 테스트 추가

**Objective:** 포지션 미선택 참가자가 있어도 전환 가능해야 한다는 contract를 고정한다.

**Files:**
- Frontend: `apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/page.test.ts`
- Backend: `apps/api/src/repository/bot_workflow.rs` 또는 `apps/api/src/http/bot_routes.rs`

**Current likely blocker:**
- Frontend `createAuctionRecruitment()` 조건에 `hasCompleteAssignments` 포함.
- Backend에 `auction_declared_lane_required` 에러 경로 존재.

### Task 3.2: Frontend 전환 버튼 조건 완화

**Objective:** 포지션 선택/팀 배정 완료 상태와 무관하게 전환 버튼이 동작하게 한다.

**Files:**
- Modify: `apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/+page.svelte`

**Implementation:**
- `createAuctionRecruitment()` guard에서 `!hasCompleteAssignments` 제거 또는 경매 전환 전용 조건으로 분리.
- 버튼 disabled/help text도 동일하게 조정.
- 일반 시리즈 생성은 기존 complete assignment 요구 유지.

**Verification:**
- entry page test에서 미배정/라인 미선택 상태에서도 `createAuctionRecruitmentFromEntryDraft` 호출 확인.

### Task 3.3: Backend declared_lane optional 허용

**Objective:** 전환 시 declared lane 없는 참가자를 경매 모집 후보로 포함한다.

**Files:**
- Modify: `apps/api/src/repository/bot_workflow.rs`
- Modify: `apps/api/src/http/bot_routes.rs` error mapping if needed

**Implementation direction:**
- 전환 시 auction participant `declared_lane`를 nullable로 저장/전달.
- 이후 경매 준비 화면은 이미 `declaredLaneLabel(null) => 미설정` 처리 가능.
- 필요한 경우 auction setup snapshot schema/DTO가 nullable인지 확인 후 맞춘다.

**Verification:**
- `auction_declared_lane_required`가 일반내전→경매 전환에서는 발생하지 않는 테스트 추가.
- 기존 직접 경매 모집 참가자 추가에서 라인 required를 유지할지 별도 판단. 요구사항은 “전환 기능”이므로 우선 전환 경로만 완화.

---

## Phase 4: 경매 잔여 인원 Button Action → Modal display

### Task 4.1: 잔여 인원 modal test 추가

**Objective:** 잔여 인원 정보가 상시 노출이 아니라 버튼 클릭 후 modal로 표시된다.

**Files:**
- Modify/Test: `apps/web/src/lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte`
- 필요 시 Test: 관련 component/page test

**Expected behavior:**
- 작업대에 `잔여 인원` 버튼 표시.
- 클릭 시 Dialog open.
- modal 안에 queued/current/unsold 후보 목록 표시.
- 각 후보는 이름, 라인, 상태를 표시.

### Task 4.2: Modal 구현

**Objective:** 기존 Dialog 패턴을 재사용해 잔여 인원 modal을 만든다.

**Files:**
- Modify: `apps/web/src/lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte`

**Implementation:**
- state: `remainingDialogOpen = $state(false)`
- button action: `remainingCount` badge 포함
- Dialog body:
  - 현재 대상 ACTIVE
  - 대기 QUEUED
  - 유찰 UNSOLD
- 기존 `remainingCount`, `queuedCandidates`, `unsold`, `playerLaneLabel()` 재사용.

**Verification:**
- `pnpm --filter @monkeybot/web test -- AuctionTournamentBidDraftWorkbench` 또는 해당 route test.

---

## Phase 5: 경매 점수 timer/progress bar

### Task 5.1: 위치 협의용 화면 확인

**Objective:** progress bar를 넣기 전 실제 레이아웃에서 가장 덜 거슬리는 위치를 확정한다.

**Files:** 없음 또는 스크린샷 산출

**Steps:**
1. 로컬 web 실행.
2. 경매 draft 화면 진입.
3. 현재 대상 패널/팀 액션 영역/헤더 하단 후보 위치를 캡처.
4. 안 A/B/C 중 선택 요청.

**Do not implement before:** 사용자가 위치를 확정하기 전에는 최종 위치 고정 금지.

### Task 5.2: Timer state unit test 추가

**Objective:** 점수 증감 시 10초 timer가 초기화되는 동작을 검증한다.

**Files:**
- Test: `apps/web/src/lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte` 관련 test

**Expected behavior:**
- currentTarget이 생기면 10s progress 시작.
- `adjustBid(team, +delta)` 호출 시 10s로 reset.
- `adjustBid(team, -delta)` 호출 시 10s로 reset.
- `finalizeBid`, `drawNext`, snapshot target 변경 시 timer cleanup/reset.
- disabled 상태/무효 입력에서는 reset하지 않음.

### Task 5.3: Progress bar 구현

**Objective:** 경매 점수 전환 이후 줄어드는 progress bar를 구현한다.

**Files:**
- Modify: `apps/web/src/lib/components/workflow/AuctionTournamentBidDraftWorkbench.svelte`

**Implementation direction:**
- constants:
  - `const BID_TIMER_DURATION_MS = 10_000`
  - tick interval 100ms 또는 `requestAnimationFrame`
- state:
  - `bidTimerStartedAt`
  - `bidTimerRemainingMs`
  - cleanup handle
- derived:
  - `bidTimerPercent = remaining / duration * 100`
- reset callback:
  - `resetBidTimer()` called inside valid `adjustBid(...)`
- UI:
  - selected position from Task 5.1
  - `role="progressbar"`, `aria-valuemin=0`, `aria-valuemax=10`, `aria-valuenow`
  - label: `입찰 조정 타이머 10초`

**Important:** 타이머는 자동 입찰/확정 같은 side effect를 만들지 않는다. 시각적 운영 보조만 한다.

**Verification:**
- fake timers 기반 unit/component test.
- 접근성 속성 확인.

---

## Phase 6: 통합 검증

### Task 6.1: Focused frontend tests

Run:

```bash
pnpm --filter @monkeybot/web test -- \
  apps/web/src/routes/g/[guildId]/auction/page.test.ts \
  apps/web/src/routes/g/[guildId]/recruitments/[id]/entry/page.test.ts \
  apps/web/src/lib/api/auctionRecruitment.test.ts
```

Expected: PASS.

### Task 6.2: Focused backend tests/check

Run:

```bash
cargo test -p monkeybot-api hard_fearless
cargo test -p monkeybot-api auction_recruitment
cargo check -p monkeybot-api
```

Expected: PASS.

### Task 6.3: Diff hygiene

Run:

```bash
git diff --check
git status --short
```

Expected:
- whitespace error 없음.
- 기존 unrelated dirty `apps/web/**` 변경과 이번 변경을 구분 가능.

---

## Rollout Notes

- DB migration은 기본 계획에 포함하지 않는다.
- 배포 전 사용자가 progress bar 위치를 확정해야 한다.
- 배포 전 기존 unrelated dirty web 변경과 이번 개선 변경을 반드시 분리 검토한다.
- 배포는 별도 지시가 있을 때만 진행한다.

---

## Suggested Implementation Order

1. 애니메이션 미동작 운영 버그 — 최우선, 다른 경매 개선보다 먼저 root cause 조사/수정.
2. 피어리스 기본값 true — 가장 작고 독립적.
3. 일반내전 → 경매내전 전환 조건 완화 — 운영 막힘 해소 효과 큼.
4. 경매 모집 생성 예상 시작 시간 required — API/UI contract 동시 변경.
5. 잔여 인원 modal — UI 독립 개선.
6. Timer/progress bar — 위치 확정됨: 작업대 헤더 하단 full-width thin bar.
