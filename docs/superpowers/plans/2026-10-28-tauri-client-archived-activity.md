# Tauri client: archived session Activity (`zerus-5wmh.42`)

**Goal:** selecting a loaded archived row shows its recorded Activity, read as
the Qt client reads it, through the existing Activity components and read-only.

**Spec:** `docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md` ("Live rows
only; Archive stays a keyset-paged read"). It builds on the Archive view
(`docs/superpowers/plans/2026-10-28-tauri-client-session-menu-parity.md`,
Piece D) and the phase 2 Activity follow (`client/src/store/activity.ts`).

## Qt behaviour

- **The read** (`HgsClient::requestInspection`, `tray/src/HgsClient.cpp:918-946`):
  - The command is `hgs [@host] inspect NAME --after CURSOR [--skip-processes] --archive ID`, with a 5 s timeout here and 15 s on a peer.
  - One read is in flight per host, name and archive id.
  - A reply that does not parse fails with "Invalid activity response"; any other failure gives its detail.
- **When it reads** (`SessionsWindow::inspect`, 3467-3492, and the 500 ms timer at 2013-2016):
  - An archived row is never streamed (`archiveId.isEmpty()` is part of `streamed`), so the selected archived row is polled.
  - The poll runs while the window shows and the Sessions page is current, every `m_activityPollMs` (2500 ms).
  - With the Terminal or native UI tab current it waits the background process interval instead, but this client has no such tabs on archived rows.
  - The cursor starts at 0 when the selection changes (451).
- **Merging the reply** (`acceptInspection`, 3587-3660):
  - The reply applies only while the same host, name and archive id are selected.
  - `replace_events` replaces the events.
  - Otherwise events with `seq <= cursor` or an already-known seq are skipped, and the rest are appended in seq order. The cursor advances to the highest seq.
  - Another conversation id than the shown one, unless it is listed in `cleared_conversations`, resets the window to the reply's events and the cursor to 0.
- **Failures** (1910-1914): the Activity shows "Could not refresh activity: {error}".
- **The pane** (3783):
  - An archived row counts as `recorded`: no live process count, no live tool line, no Clear session.
  - Qt has no older-history fetch for inspected sessions; the first reply's window is all it shows.

## Owner-session constraints (`claude/zerus-work-e94411a3/webui`, 2026-10-28)

1. The live follow stays untouched. Archived events come from a separate bounded read, not the watch stream.
2. They render through the existing Activity components and event shape, with no forked renderer. The pane is read-only: no composer, no send or interrupt affordances, no live indicators. The phase 4 composer hides itself for `archive:` keys and checks this piece's flag.
3. Archived Activity has its own store (`client/src/store/archiveActivity.ts`), keyed by the archive key and dropped when the selection leaves it. `store/activity.ts` keeps its live-follow logic as it is.

The store and component touch points go to the owner session before the PR.

## Global Constraints

- **Qt wins.** Where this plan and Qt disagree, Qt is right; fix the plan in the same change.
- Add nothing Qt does not have.
- No dots as UI separators (`scripts/check-ui-separators.py`), no keyboard hints.
- **Bounded data:**
  - one archived row's events at a time;
  - its reply is read with the core's `READ_LIMIT`;
  - nothing is kept after the selection leaves the row.
- Tests never run the real `hgs`; the fake is `client/core/tests/support/fake_hgs_actions.py`, or a fake reads script as the reads tests use.
- **Shared files:**
  - `client/src/ui/sessions/Activity.tsx` gains one optional source input on `ActivityView`, and the read-only gating around the live indicators;
  - `client/src/ui/sessions/SessionDetail.tsx` chooses the source;
  - everything else is new files, or additive in `types.ts`, `bridge.ts`, `reads.rs` and src-tauri.

## Task G1: the read (core and src-tauri)

- In `client/core/src/reads.rs`, add `Read::ArchivedActivity { name: String, archive_id: String, after: i64 }`:
  - arguments `inspect NAME --after AFTER --skip-processes --archive ID`, with an `@host` prefix for a peer;
  - timeouts 5 s here and 15 s on a peer;
  - output up to `READ_LIMIT`.
- An empty name or archive id fails before any call, with the same text the other archive refusals use.
- A reply that is not a JSON object fails with "Invalid activity response". Other failures give the core's failure detail, mapped as the Archive read maps it to Qt's `failureDetail` texts (`timed out`, `cannot start '{hgs}': {error}`, the trimmed standard error up to 4000 characters, or `exit code {n}`).
- The typed reply:
  - `events`: the array as `ActivityEvent` values, unchanged;
  - `replace_events`: a boolean;
  - `conversation_id`, `run_id`, `cleared_conversations`, and `cursor` (Qt's `data.cursor`, the next `--after`);
  - `summary`: the remaining top-level fields the transcript builder reads from a live `state` value (`buildTranscript(events, summary)`).
- In `client/src-tauri`, add `archived_activity(computer, name, archive_id, after)`. It runs on the blocking pool, refuses in replay mode without `ZERUS_CLIENT_HGS` (as the other reads do), and allows one read in flight per computer, name and archive id. A second read answers "busy", which the store ignores, as Qt's `return false`.
- In `bridge.ts`, add `archivedActivity(...)`, with a browser fallback that rejects. Add the types to `types.ts`.
- Tests: the arguments and timeouts, the refusals, a fake reply (events, `replace_events`, conversation change), invalid JSON, and an exit with and without standard error.

Commit `feat(client): the archived Activity read`.

## Task G2: the store and the pane

- **The store** (`client/src/store/archiveActivity.ts`), a port of Qt's merge in `acceptInspection` (3639-3660):
  - `createArchiveActivity(deps)`, where `deps` is the bridge call, a clock or timer seam, and whether the Sessions page shows. The app instance follows the selected row: active only for an `archive:` key whose loaded archived row has an archive id.
  - It implements `ActivitySource`, an exported interface in `client/src/store/activitySource.ts` that both this store and the live `activity` store implement: `target`, `events`, `summary`, `loaded`, `loadingOlder` (always false here), `olderDone` (always true here), `ended` (false here), `loadOlder` (a no-op here), `error` and `readOnly` (true here). Phase 4 extends only the live store's side. A source that lacks something renders nothing for it.
  - On a new selection it resets the events, summary and cursor and the error, and reads with `after = 0`.
  - It polls every 2500 ms only while the row stays selected and the page shows, with no overlapping read. A "busy" answer (`ARCHIVED_ACTIVITY_BUSY`) is ignored, as Qt's `return false`.
  - A reply for another key than the selected one is dropped.
  - Leaving the row, or closing the window, stops the timer and drops everything.
  - `ARCHIVED_EVENTS_MAX = 5000` caps the events, dropping the oldest first. It guards against a runaway poll and has no Qt counterpart: Qt keeps what it loaded, and an archived session does not grow. One test pins the drop.
  - Replies merge as Qt does:
    - `replace_events` replaces the events;
    - otherwise new events with a seq above the cursor are appended in seq order;
    - a conversation change (not in `cleared_conversations`) resets the events to the reply's and the cursor to 0.
  - A failure sets `error` to "Could not refresh activity: {detail}"; the next good reply clears it.
- **`ActivityView`** (`Activity.tsx`), with the read-only branching kept as small as possible:
  - It takes an optional `source` with that surface, defaulting to the live `activity` store.
  - With `source.readOnly`, it shows the transcript instead of the "This conversation is archived." notice, and it shows no live tool line, no attention or error banners for live states, no offline notice and no "left the list" notice.
  - It shows `source.error` as a notice when set.
  - The older-page scroll trigger never fires for it (`olderDone` is true).
- **`SessionDetail.tsx`:** for a selected archived row, pass the archived store as the source. Export `activityReadOnly(key)` (true for `archive:` keys) for the owner session's composer, either from here or from the store. Archived rows show no Terminal tab: Qt has none for them, and `terminal_status` already answers unsupported.
- **Tests:**
  - the store: reset on selection, the poll cadence and no overlap, the merge rules, a conversation change, a dropped late reply, the error text and its clearing, and nothing kept after leaving;
  - `ActivityView` with a fake read-only source: it renders items, and shows none of the live indicators;
  - `SessionDetail` picks the archived source only for `archive:` keys and shows no Terminal tab for them.
- **README** (`client/README.md`, the "Archive" section): one paragraph on archived Activity.
- **Smoke:** after the archive inspect step, the actions smoke logs `actions-smoke archive-activity: done (N events)` once the archived row's Activity shows the fake's events. The README config gives the fake's `inspect` entry for that row one or two synthetic events.

Commit `feat(client): archived Activity, read-only through the Activity view`.

## Review Focus

1. The live follow and `store/activity.ts` are unchanged; archived rows never subscribe to the stream.
2. Nothing is read or kept for a row that is not selected; every read is bounded.
3. The merge never duplicates or reorders events, and a late reply for another row never shows.
4. The read-only pane shows no live affordance.
