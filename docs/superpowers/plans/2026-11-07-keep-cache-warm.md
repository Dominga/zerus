# Keep Cache Warm Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A per-session Keep cache warm setting whose pings are sent by the
per-computer recovery worker and stay silent everywhere a person's message
would surface.

**Architecture:** A new `src/state/keep_warm.rs` owns the setting store, the
eligibility and timing rules, the worker round, the submission check and the
journal marks. Existing modules get narrow hooks: `input::submit_core` gains a
keep-warm origin, `hooks.rs` calls `keep_warm::observe`, `journal.rs` tags and
skips ping turns, `recovery.rs` runs the round and ignores ping failures. The
desktop reads the listing/inspect fields and calls `hgs keep-warm`.

**Tech Stack:** Rust (hgs, client core, Tauri), SolidJS/TypeScript frontend.

**Spec:** `docs/superpowers/specs/2026-11-07-keep-cache-warm-design.md`

## Global Constraints

- Off by default; `until` is the agent computer's clock; `null` is Always.
- Hours 1 to 7 or Always; at most 64 enabled sessions per computer.
- Ping text exactly: `[Zerus keep-warm] Reply with exactly OK and do nothing else.`
- Interval `0.8 × ttl`; never ping within 30 s of or after the estimated expiry.
- Offered only for TTL ≥ 15 min: Claude (observed TTL), Codex GPT-5.6 and later
  (1800 s), Kimi (`estimated_cache_duration`, fallback table). Others show a
  disabled switch with a one-line reason.
- Unknown delivery never authorizes a replay.
- Tests use private `HGS_CONFIG_DIR`/`HGS_STATE_DIR`, private tmux sockets and
  fake agents; cargo targets stay in the worktree.
- UI never separates values with dots; Russian strings in
  `client/src/i18n.next-ru.json`.

## Review Focus

1. The person sends a message while a ping is dispatching or running: their
   message must be delivered normally and the turn must become theirs (reply,
   unread and notification behave as for any message).
2. The computer sleeps past the estimated expiry: on wake no ping is sent;
   keep-warm reports a cold cache and resumes after the next real turn.
3. Two desktops change the setting at once: one wins by revision, the other
   gets `conflict`; the single executor never sends two pings for one window.
4. The session is renamed, paused, resumed, cleared or archived while enabled:
   the setting follows the session ID; pause sends nothing; archive removes it.
5. A ping turn fails (usage limit, sign-in, network): no automatic recovery
   continuation, keep-warm stops with the reason, the native failure stays
   visible.

---

## PR 1: hgs

### Task 1: Setting store and `hgs keep-warm`

**Files:**
- Create: `src/state/keep_warm.rs`
- Modify: `src/state/mod.rs` (module, dispatch `keep-warm`, listing enrich next
  to `notifications::enrich`), `src/state/journal.rs:1131` (inspect enrich),
  `src/state/watch_source.rs:276`, `src/cli.rs:1297` (marker
  `session-keep-warm-v1`, help line)

**Interfaces:**
- Produces: `keep_warm::Store`, `keep_warm::load() -> Option<Store>`,
  `keep_warm::enrich(session: &mut Value, store: Option<&Store>, record: &Value)`,
  `keep_warm::forget(record: &Value)` (archive/terminate),
  `keep_warm::TEXT: &str`.
- Store file `keep-warm.json`:
  `{"version":1,"sessions":{"<owner_key>":{"name":..,"enabled":bool,"until":f64|null,"revision":u64,"set_at":f64}}}`
- Command: `hgs keep-warm SESSION --json`, stdin
  `{"expected_run_id","expected_conversation_id","expected_revision","enabled","hours"?:1..7,"always"?:bool}`;
  output `{"ok":true,"applied":bool,"keep_warm":{...}}` or
  `{"ok":false,"error":"stale_identity|conflict|capacity|invalid"}`.

- [ ] Write reducer tests: enable with hours 3 sets `until = now + 10800`;
  `always` sets `until: null`; disable keeps revision+1 and `enabled:false`;
  stale identity, revision conflict, 65th enabled session (`capacity`),
  hours 0 and 8 (`invalid`), no-op returns `applied:false`.
- [ ] Implement store (bounded 1 MiB read, `deny_unknown_fields`, atomic
  write under `keep-warm.lock`), reducer, dispatch (identity via `expect` and
  `notifications::current`), `enrich` (stored setting + runtime state +
  `turn_active` from `record["keep_warm_turn"]`).
- [ ] Wire listing/inspect/watch enrich, capability marker, archive/terminate
  `forget`.
- [ ] `cargo test keep_warm`; commit.

### Task 2: Eligibility and timing

**Files:** `src/state/keep_warm.rs`

**Interfaces:**
- Produces: `fn eligibility(summary: &Value, record: &Value, now: f64) -> Eligibility`
  with fields `supported: bool, reason: &'static str, ttl: u64, source: &'static str,
  expires_at: Option<f64>, due_at: Option<f64>`; inspect adds
  `supported, reason, ttl_seconds, ttl_source, interval_seconds`.

- [ ] Tests: Claude with `prompt_cache {ttl_seconds:3600, expires_at:E}` →
  supported, due `E - 3600 + 2880`; Claude 300 → unsupported with the 5-minute
  reason; Claude with no observation → supported but waiting; Codex model
  `gpt-5.6`, `gpt-6.1-sol` → 1800 from last Stop; `gpt-5.5` → unsupported;
  Kimi `kimi-for-coding` → 3600, `k3.1-pro` → 1800, unknown → unsupported;
  Kimi config file overrides the table; `dsh`, `opencode` → unsupported.
- [ ] Implement; commit.

### Task 3: Keep-warm submission origin

**Files:** `src/state/input.rs:960-1275`, `src/state/keep_warm.rs`

**Interfaces:**
- Produces: `input::submit_keep_warm(name: &str, input: &[u8], id: &str) -> Submission`
  (returns the existing `Submission` enum so the worker can tell NotOffered
  from Unknown); `keep_warm::check_submission(record: &Value, id: &str) -> Result<()>`.

- [ ] Tests for `check_submission`: refuses when not settled, schedules > 0,
  live background jobs, `input_pending_at`, recovery job, launch prompt
  pending, effort pending, revision changed, expired, pending id mismatch.
- [ ] Implement: an `Origin` parameter replacing `recovery_id`/`answer`
  (`User`, `Answer`, `Recovery(id)`, `KeepWarm(id)`); KeepWarm refuses pending
  settings, skips `recovery::cancel_for_input`, re-runs `check_submission`
  under the lock and immediately before input, writes `"origin":"keep_warm"`
  in the receipt and `record["keep_warm_pending"] = {request_id, submitted_at}`.
- [ ] Existing input tests stay green; commit.

### Task 4: Silent ping turns in the journal

**Files:** `src/state/keep_warm.rs`, `src/state/hooks.rs:1424`,
`src/state/journal.rs` (`log_event`, `update_activity`, `latest_reply`,
`kimi_reply`), `src/state/recovery.rs` (episode start), `src/state/search_index.rs`

**Interfaces:**
- Produces: `keep_warm::observe(record: &mut Value, event: &mut Value)` (before
  `update_activity`), `keep_warm::settle(record: &mut Value, event: &Value)` (after
  it). Marks: `event["keep_warm"]="prompt"|"turn"`; journal payload
  `origin:"keep_warm"` on the prompt and `turn_origin:"keep_warm"` on the turn's
  main events.

- [ ] Tests: prompt equal to `TEXT` with pending ping → origin set, record
  `keep_warm_turn`; following Stop tagged, `last_message` and `last_event_at`
  unchanged, `keep_warm_turn` cleared; a different prompt mid-turn clears the
  mark and the Stop is an ordinary reply; `latest_reply` returns the previous
  real Stop (Claude/Codex SQL and Kimi); a `StopFailure` in a ping turn creates
  no recovery job and stops keep-warm; prompt text without a pending ping is
  ordinary.
- [ ] Implement; commit.

### Task 5: Worker round

**Files:** `src/state/keep_warm.rs`, `src/state/recovery.rs:892-925, 960`

**Interfaces:**
- Produces: `keep_warm::tick(now: f64) -> Result<()>` and pure
  `fn decide(entry, runtime, summary, eligibility, now) -> Decision`
  (`Wait(reason)`, `Expire`, `Cold`, `Send`); runtime file
  `recovery/keep-warm/<owner>.json`.

- [ ] Table tests for `decide`: expired, not due, cold (past expiry), within
  30 s of expiry, busy, background jobs, schedules, uncertain previous ping,
  attached client typed 30 s ago, due and idle → `Send`.
- [ ] Implement `tick` (every 15 s from the worker loop, independent of the
  recovery policy; `ensure_worker` on enable), result handling (NotOffered →
  waiting; Unknown → uncertain; receipt → warming), expiry write.
- [ ] Integration test with a fake agent on a private tmux socket: enable,
  force due, one ping submitted with the exact text and origin, no second ping
  in the same window; commit.

### Task 6: Docs

- [ ] `docs/reference.md`: a Keep cache warm section (Activity, messages and
  native controls) and the `keep-warm` command under JSON and external
  clients; mention the recovery worker. Commit; open PR 1.

## PR 2: desktop

### Task 7: Core and bridge

**Files:** `client/core/src/view.rs` (ViewRow `keep_warm`, quiet status),
`client/core/src/actions.rs` (`change_keep_warm`, invocation with `@host`),
`client/src-tauri/src/actions.rs`, `client/src-tauri/src/lib.rs`,
`client/src/bridge.ts`, `client/src/store/catalog.ts`,
`client/src/ui/sessions/types.ts`, `client/src/ui/sessions/transcript.ts`.

- [ ] Tests: core status Ready + "Warming cache" while `turn_active`; filter
  membership unchanged; invocation builds `hgs @host keep-warm NAME --json`;
  transcript folds a keep-warm prompt, its replies and its Stop into one
  `cache` row; a mid-turn person prompt stays a normal user row.
- [ ] Implement; commit.

### Task 8: Details row

**Files:** `client/src/ui/sessions/Inspector.tsx`, a new
`client/src/ui/sessions/KeepWarmRow.tsx`, styles, `client/src/i18n.next-ru.json`
(+ regenerated `i18n.ru.json`).

- [ ] Tests: component test for disabled reason, range 8 = Always, remaining
  text "2 h 14 m left", switch accessible name; i18n coverage;
  `scripts/check-ui-separators.py`.
- [ ] Implement; commit; open PR 2.
