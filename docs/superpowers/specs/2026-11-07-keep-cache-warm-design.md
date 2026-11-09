# Keep cache warm

Epic `zerus-ryeca`. Owner request, 2026-11-07: a per-session **Keep cache warm**
switch in the desktop Details panel, **Session** card, with a duration of 1 to
7 hours or Always. While it is on, Zerus keeps the agent's provider prompt cache
warm by sending a minimal message to the idle agent shortly before the cache
would expire, so the next real message is cheap and fast.

This note covers the cache lifetime of each supported agent, the delivery
mechanism, safety rules, timer ownership, data model, presentation and the
out-of-band alternative.

## Cache lifetimes

Researched 2026-11-07 against the installed agents (Claude Code 2.1.319,
codex-cli 0.167.0, Kimi Code 2.5.2) and provider documentation. Every provider
below refreshes a cache entry when a request reads it, which is what makes a
ping useful.

| Agent | Lifetime Zerus uses | Source | Keep-warm |
|---|---|---|---|
| Claude Code | Observed per request: 1 h or 5 min | native usage `cache_creation.ephemeral_1h_input_tokens` / `ephemeral_5m_input_tokens`, already parsed in `src/state/usage.rs` | 1 h: yes; 5 min: not offered |
| Codex, GPT-5.6 and later | 30 min | OpenAI server default `prompt_cache_options.ttl = "30m"`; Codex sends only `prompt_cache_key` | yes |
| Codex, GPT-5.5 and earlier | 5 to 10 min in memory, undocumented refresh | OpenAI prompt caching guide | not offered |
| Kimi Code | 60 min for `kimi-for-coding*`, 30 min for `k3*` | Kimi Code's own `estimated_cache_duration` client config | yes |
| DeepSeek (dsh) | hours to days, best effort | DeepSeek KV cache guide | not needed |
| OpenCode and others | unknown | none | not offered |

Details and citations:

- **Anthropic.** "By default, the cache has a 5-minute lifetime"; the optional
  1-hour TTL is `cache_control: {type: "ephemeral", ttl: "1h"}`. "The cache is
  refreshed for no additional cost each time the cached content is used."
  Writes cost 1.25x (5 min) or 2x (1 h) of input; reads cost 0.1x (0.05x on
  Opus 5.5 and Sonnet 5.5).
  <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>
- **Claude Code.** "Claude Code requests the one-hour TTL only on a Claude
  subscription within your plan's included usage"; API keys, cloud providers
  and usage credits get 5 minutes. Overrides: `FORCE_PROMPT_CACHING_5M`,
  `CLAUDE_CODE_PROMPT_CACHE_TTL`, settings `promptCacheTtl`,
  `ENABLE_PROMPT_CACHING_1H`.
  <https://code.claude.com/docs/en/prompt-caching.md>. Claude Code also
  compacts an idle conversation of at least 200k tokens at 90% of its TTL
  (about 54 minutes for 1 h); `idleCompaction: false` disables it. Zerus does
  not guess the TTL from the account kind: it uses the TTL written by the last
  request, so it follows a subscriber who moves onto usage credits.
- **OpenAI.** For GPT-5.6 and later, `prompt_cache_options.ttl` defaults to
  `"30m"` and "A 30-minute read refreshes coverage to at least 30 minutes after
  that read"; writes cost 1.25x and reads 0.05x to 0.1x. GPT-5.5 and earlier
  keep in-memory entries for "around 5 to 10 minutes of inactivity, up to one
  hour" with no documented refresh rule.
  <https://developers.openai.com/api/docs/guides/prompt-caching#summary-of-model-differences>.
  Codex 0.167.0 sends `prompt_cache_key` (the session ID) and no TTL, so the
  server default applies. ChatGPT-subscription sessions report
  `cached_input_tokens` in native usage.
- **Kimi.** The Kimi API refreshes a hit "under the original TTL, at no extra
  cost" (<https://platform.kimi.ai/docs/guide/use-context-caching-feature-of-kimi-api>).
  The real lifetime on the Kimi Code endpoint is not documented; Kimi Code
  itself uses Moonshot's `estimated_cache_duration` client configuration (fetched
  2026-11-07: 3600 s for `kimi-for-coding` and its high-speed variant, 1800 s for
  `k3`, `k3-256k`, `k3.1-pro` and its high-speed variant) to open its "Cache
  expired" dialog when a prompt is submitted after that idle time. Zerus reads
  the same file from the session's Kimi home (bounded to 64 KiB) and falls
  back to these values; a model with no rule is not offered.
- **DeepSeek.** Disk cache, on by default and best effort: "usually within a
  few hours to a few days" after last use; hits cost 2 to 3% of a miss.
  <https://api-docs.deepseek.com/guides/kv_cache>. A ping adds nothing useful,
  so the row explains that DeepSeek keeps its cache without pings.

## Mechanism

A keep-warm **ping** is one ordinary guarded terminal submission of a fixed
text:

```text
[Zerus keep-warm] Reply with exactly OK and do nothing else.
```

The bracketed prefix follows the existing `[HGS automatic recovery]`
convention. The provider serves the whole conversation prefix from cache, which
refreshes its lifetime, and appends two short messages (about 40 tokens) to the
conversation. A 1-hour cache kept warm for a day adds about 24 such pairs, well
under 2,000 tokens.

### Interval

`interval = 0.8 × ttl`, measured from the end of the last main-agent turn (a
real turn or a ping): 48 minutes for a 1-hour cache and 24 minutes for a
30-minute cache. The 20% margin keeps pings ahead of Claude Code's idle
compaction at 90% of the TTL, so a session the person chose to keep warm is not
compacted while idle, and absorbs worker scheduling delay. The estimated
expiry is `session_usage.prompt_cache.expires_at` for Claude (the time of the
last observed request plus its written TTL) and the last main-agent Stop plus
the TTL for Codex and Kimi, which is also the idle clock Kimi's own dialog
uses. A ping is never
sent after the estimated expiry: once a cache is presumed cold, a ping would
pay the full rebuild price, exactly what the next real message would pay, and
for Kimi it would open the native cache-expiry chooser. Keep-warm therefore only
**extends** a warm cache. After a missed window (computer asleep, agent busy
until expiry, an unconfirmed delivery) it waits for the next real turn and then
continues.

Keep-warm is offered only when the TTL is at least 15 minutes. Below that a
ping is due every few minutes: with Claude's 5-minute cache that is 15 pings an
hour, each a cached read of the whole context (0.05x to 0.1x of an uncached
read), so one idle hour costs about as much as rebuilding the cache once (1.25x)
and adds 30 messages to the conversation. The row explains this instead of
offering the switch, and names the native lever: Claude Code's
`CLAUDE_CODE_PROMPT_CACHE_TTL=1h` or `promptCacheTtl` setting requests the
1-hour cache on API-key accounts at a 2x write price.

Cost per idle hour with a 1-hour cache: 1.25 pings, each a cached read of the
whole context plus a write of about 40 new tokens. A 300k-token Claude context
on Opus 5.5 therefore costs about 0.06x of one uncached 300k read per hour,
against 2x for rebuilding the 1-hour cache after it expires.

## Timer ownership

The timer runs in the existing per-computer **recovery worker**
(`hgs recovery worker`, `src/state/recovery.rs`), on the computer that runs the
agent:

- It runs without any GUI: the installed `hgs-recovery.service` (systemd) or
  `com.hgdev.hgs-recovery` (launchd) job keeps it alive; on Windows the session
  host supervises it; otherwise a detached worker is the fallback.
- One executor per computer: `service_worker::acquire` holds
  `recovery/worker.lock`. Desktops and phones only write the setting; the
  executor alone sends pings, so two open desktops never double-send.
- It restarts itself when its executable is replaced, so installed updates
  apply without restarting agents, tmux or DeepSeek hosts.
- Enabling keep-warm calls the same `ensure_worker` as a recovery policy
  change. The keep-warm round runs every 15 seconds in the worker loop,
  independent of the recovery policy's enabled flag.

The desktop background service was considered and rejected: it is per GUI
computer, cannot reach sessions on peers, and stops when ownership returns to
Native.

## Data model

### Setting

A private `keep-warm.json` beside `notifications.json`, following the shared
Mute store pattern (`src/state/notifications.rs`):

```json
{"version":1,"sessions":{"<owner>":{"enabled":true,"until":1794100000.0,
  "revision":3,"set_at":1794089200.0}}}
```

- Keyed by the session's owner key, not by conversation: the person enables it
  for the session; a new conversation after Clear keeps the setting while the
  worker's runtime state starts over.
- `until` is an absolute time on the **agent computer's clock**; `null` means
  Always. The client sends `hours` (1 to 7) or `always`, never a client-clock
  deadline.
- Writes compare `expected_revision`; stale, unknown-identity and capacity
  refusals follow the notifications store. At most 64 sessions per computer may
  be enabled, which bounds every read of the store.
- Archive, termination and session removal delete the entry. Pause keeps it
  (no live agent, so no pings); resume continues until `until`.

### Runtime state

Worker-owned `recovery/keep-warm/<owner>.json`: `state` (`waiting`,
`dispatching`, `warming`, `cold`, `uncertain`, `stopped`, `unsupported`),
`request_id`, `last_ping_at`, `next_at`, `pings` and a short `reason`. History
is not retained beyond the latest ping.

### CLI and listing

```sh
hgs [@host] keep-warm SESSION --json   # stdin: identities + enabled + hours|always
```

The request carries `expected_run_id`, `expected_conversation_id`,
`expected_revision`, `enabled`, and `hours` or `always`. Each `hgs ls --json`
row carries only the stored setting and the worker state, two bounded reads:

```json
"keep_warm":{"enabled":true,"until":1794100000.0,"revision":3,
  "state":"waiting","next_at":1794092440.0,"turn_active":false}
```

The command response and `hgs inspect` (the Details read) add the derived
fields: `supported`, `reason`, `ttl_seconds`, `ttl_source` (`native_usage`,
`provider_default`, `kimi_config`) and `interval_seconds`.

`hgs --help` advertises `session-keep-warm-v1`; clients check it before showing
the control for a peer, as for notifications.

## Safety rules

The worker sends a ping only when every rule holds, checked from fresh state
immediately before the submission; `input::submit_core` repeats its own checks
under the input lock.

1. The setting is enabled, not expired, and its revision is unchanged since the
   decision. At `until` the worker turns it off and records `expired`.
2. The agent is supported, its TTL is known, the ping is due, and the estimated
   cache expiry is still at least 30 seconds away.
3. The exact verified process is live with the expected run and conversation;
   the session is not paused, pausing, launching, archived or terminated.
4. The agent is settled (`telemetry::settled`), has no live background jobs
   (desktop **Background**), no scheduled jobs, no recovery job, no unconfirmed
   input (`input_pending_at`), no native queued messages and no pending
   launch prompt.
5. No question, approval, trust, resume, usage-limit, permission, cache-expiry
   or terminal chooser is pending (`checked_input`), and no pending model or
   effort setting would be typed first (as for question answers).
6. The composer is empty and recognized (`terminal_check` for sending).
7. Nobody is typing: no attached terminal client has had input in the last two
   minutes. Backends that cannot report client input time treat any attached
   client as in use.
8. The previous ping was not left uncertain.

Delivery reuses the guarded path with a new submission origin, like the
existing `recovery_id`: a fresh request ID, the receipt marked
`"origin":"keep_warm"`, no cancellation of recovery, and the ping recorded on
the session record as `keep_warm_pending`. A refusal proven before input keeps
the state `waiting`. Any unknown outcome sets `uncertain`; keep-warm then waits
for the next real turn and never replays.

## Silent ping turns

A ping is not the person's message.

- **Journal.** The provider's prompt event (`UserPromptSubmit`, Kimi
  `TurnStarted`) whose text equals the constant while `keep_warm_pending` names
  a submitted ping gets `origin: "keep_warm"`, and the record holds
  `keep_warm_turn`. Main-agent events until that turn's Stop get
  `turn_origin: "keep_warm"`. Any other prompt during the turn (a message the
  person queued) clears the mark: the turn is then the person's.
- **Reply, unread and notifications.** `latest_reply` skips Stops with
  `turn_origin: "keep_warm"`, so `reply_id` stays on the last real reply: no
  unread badge, no "Agent has a new reply" notification, no read-mark movement.
- **Ordering.** Keep-warm events do not advance `last_event_at`, so the
  session does not jump to the top of time-sorted lists.
- **Status.** While `keep_warm.turn_active` is true the desktop shows the
  session as Ready with a quiet **Warming cache** detail, never Working or
  Background; Active and other filter membership is unchanged. Task state and
  supervised-child quiet rules are unaffected.
- **Activity.** One dimmed, collapsed row **Cache kept warm** at the ping time
  replaces the prompt and the reply; expanding it shows both.
- **Failures.** A provider failure in a ping turn never starts automatic
  recovery, and keep-warm stops until the next real turn with the failure as
  its reason. The native failure itself stays visible: an exhausted quota or
  failed sign-in would equally stop the person's next message.

## Desktop presentation

In Details, **Session** card, after **last event**:

- A row with the bolt icon, **Keep cache warm** and a switch (`role="switch"`,
  accessible name "Keep cache warm"), keyboard operable.
- When on: a range input 1 to 8 (8 = Always, accessible name "Duration") with
  its value **3 h** or **Always**, and the remaining time **2 h 14 m left** or
  **Always**. Changing the duration sets a new deadline from now.
- Tooltip: the interval, the estimated pings per hour and the cost, for
  example "Pings every 48 m while idle, about 1.25 an hour. Each ping reads the
  whole cached context (about 331k tokens) from this account's usage."
- The row is never hidden. When the agent or account is not eligible (Claude
  on a 5-minute cache, Codex on GPT-5.5 or earlier, DeepSeek, OpenCode), the
  switch is disabled and a one-line reason sits under it.
- Other states: waiting for the first cache report, **Cache went cold**
  (resumes after your next message), last delivery unconfirmed, expired.
- The ping text is fixed as above; making it configurable is a possible later
  change, not part of this work.
- A peer without `session-keep-warm-v1` shows the row disabled: "Update Zerus
  on this computer to keep the cache warm".
- Russian translations; no dot separators between values.

The existing composer chip (**Cache ~48m**, `client/core/src/cache.rs`) already
shows the estimated remaining lifetime from native usage and follows pings
automatically, so no new header chip is added.

## Out-of-band alternative

Evaluated, not built: a local API relay (`ANTHROPIC_BASE_URL`, an OpenAI base
URL) captures each agent request and, when the cache nears expiry, replays the
last request unchanged except `max_tokens: 1`. It would add nothing to the
conversation and need no idle terminal, but:

- **Credentials.** Subscription sessions authenticate with Claude.ai or ChatGPT
  OAuth tokens. The relay would hold them and send requests outside the
  official client, which the subscription terms do not cover and which can put
  the account at risk. Only API-key accounts are clean, and the relay would
  still see every token and every prompt.
- **Exactness.** A cache hit needs the exact prefix bytes: model, tools,
  system prompt, messages and cache breakpoints (thinking settings also affect
  the message cache). Any difference silently writes a new entry at 1.25x to
  2x and leaves the real one to expire. Replaying the captured last request
  reaches that prefix, but only while the relay keeps the full body of every
  session's last request (hundreds of kilobytes of private context each).
- **Reach and risk.** Agents must be launched through the relay, so existing
  sessions need a restart; the relay becomes a critical path for all streaming
  traffic. Codex uses a WebSocket Responses transport to the ChatGPT backend,
  which is not a public API.
- **Benefit.** Billing is the same as a ping minus about 40 tokens per ping:
  the input is a cached read either way.

Recommendation: keep the in-conversation ping. Revisit an out-of-band refresh
only for API-key Claude accounts if the ping's conversation entries prove a
problem; before that, the cheaper lever for those accounts is Claude Code's own
1-hour TTL setting, which a later launch option could set.

## Delivery plan

1. **hgs** (one PR): store, `keep-warm` command, listing field, worker round,
   submission origin, journal marks, reply/ordering exclusions, recovery
   exemption, `docs/reference.md`. Tests use private `HGS_CONFIG_DIR` and
   `HGS_STATE_DIR`, private tmux sockets and fake agents.
2. **Desktop** (one PR): view fields, quiet status, Activity row, Details row,
   Tauri command and bridge, translations, `scripts/check-ui-separators.py`.
3. **Follow-ups**: Android display and control; the out-of-band refresh, if
   adopted.
