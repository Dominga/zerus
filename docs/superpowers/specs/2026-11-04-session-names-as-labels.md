# Session names as labels

**Status:** Draft for owner review, 2026-11-04
**Task:** `zerus-ucck` part 2 (part 1 shipped in #1223: archived sessions never hold a name, and forks pick a free `-2`, `-3`… name)
**Source baseline:** `integrate/design-polish` at `f74615db1`

## Goal

A session name is a label. Two live or saved sessions on one computer may carry the same label, just as two archived sessions already may. Everything that must find one exact session uses an identity, not the label. Existing `hgs` syntax, `HGS_*` variables, state and config paths, and application and service identifiers stay compatible. A rename or upgrade must never reset accounts, project catalogs, native agent processes or saved conversations.

## What the label keys today

The label is unique among live and saved sessions because several layers use it as a key.

| Layer | Keyed by the label today | Stable identity next to it |
| --- | --- | --- |
| Terminal | tmux `new-session -s NAME` and `=NAME` targets; the Windows host registry refuses a duplicate name | tmux `$N` (renames already use it) |
| Records | `sha256(name).json` (or `bindings/` through `name_routes`). DeepSeek and OpenCode bindings, recovery jobs, input locks and fork intents use the same hash | `run_id` (changes on every resume), `conversation_id` (changes on clear), `launch_id` (absent on forks) |
| Hooks | `HGS_SESSION` plus `HGS_RUN_ID`: `read_run` reads by name first, then scans every record by run | `HGS_RUN_ID` |
| CLI | every `hgs NAME …` command, open-or-create launch, `@host` forwarding | `zerus://session?…` references (resolved by conversation or run), `--expected-run-id`/`--expected-conversation-id`, `hgs a --existing --run-id` |
| Journal and read marks | `events.sqlite3` rows and `read-marks.json` under `journal_name` (the name, pinned at the first rename), split by conversation | conversation |
| Organization | catalog membership `Key::Session { machine, session: NAME }`; desktop drafts and desktop read marks by computer and name | none |
| Desktop rows | `run:<run_id>`, falling back to `name:` | `run_id` |
| Mobile | relay envelope `session: NAME` with `expected_run_id`/`expected_conversation_id` | the expected IDs |

Archives, launches, `hgs wait`, tasks with a `launch_id:` or `run_id:` reference, and the phone history index are already keyed by IDs.

## Proposal

### 1. A durable session ID

Every record gains `session_id`, a UUID assigned when the session is first created. It is kept across resume, clear, rename, pause, archive and restore. A fork gets its own. Records written by older CLIs get one the next time this CLI writes them. Until then they are found by name as today. An older CLI that rebuilds a record (on resume, for example) drops the field, so the next write here assigns a new ID once; there is no recovery index (#1317 R5). `session_id` becomes the key for everything that must find one session. `run_id` and `conversation_id` stay what they are today: guards for one run and one conversation.

New records are stored under `sessions/<session_id>.json`, with a label index (label → session IDs) that replaces the name hash for lookups. Existing `sha256(name)` records and `name_routes` stay readable, and a session moves to the new layout only when this CLI rewrites it. A new session's `journal_name` is `session:<session_id>`. Existing sessions keep theirs, so their history and read marks stay where they are.

### 2. Terminal names

The terminal session gets its own name, stored in the record as `terminal_name`. A new session uses its label when no other terminal session has it, so `tmux attach -t codex/project/tag` keeps working in the common case. Otherwise it uses the label plus `~` and the first 8 characters of its session ID, for example `codex/project/tag~3f2a9c1d`. All control goes through the record's `terminal_name` or tmux's `$N`, never the label. A running session's terminal is never renamed just to match a label; a rename changes the label and, where it is free, the terminal name, as today. The Windows host follows the same rule through its registry.

`HGS_SESSION` keeps its meaning, the label at launch, for scripts and agents that read it. A new `HGS_SESSION_ID` carries the session ID, and hooks resolve by `HGS_SESSION_ID`, then `HGS_RUN_ID`, then the label.

### 3. CLI addressing

- `hgs NAME …` works unchanged whenever the label is unique on that computer, which remains the common case.
- Every command that takes a session also accepts:
  - a `zerus://session?…` reference, which already carries machine, name, conversation and run;
  - `--session-id ID`;
  - the existing `--expected-run-id` / `--expected-conversation-id` guards, which select when several sessions share the label.
- Launching with a label in use:
  - exactly one live or saved session has it: attach or resume, as today;
  - several do: refuse as ambiguous;
  - `--new`, which never attaches or resumes, keeps refusing a label in use until P3; from P3 it creates another session with that label instead.
- `@host` forwards the selectors unchanged. A peer whose `hgs` predates them refuses an ambiguous label rather than guessing.
- Machine clients (desktop, Android, iOS, the connector) already send expected run and conversation IDs. They resolve by those IDs first and add `session_id` once it exists. API v1 stays compatible: `session_id` is an optional field, and `session` stays the label.

### 4. Reporting ambiguity

There is one shape everywhere. The CLI exits with a dedicated status, 5 (status 3 already means "unreachable"), and prints:

```text
hgs: 2 sessions on this computer are named codex/project/tag. Choose one with --session-id, or use its reference:
  3f2a9c1d  live    ~/work/project   started 2026-11-04 10:12  zerus://session?…
  7be01d44  saved   ~/work/project   started 2026-11-02 18:40  zerus://session?…
```

The `--json` form is `{"error":"ambiguous_session","name":…,"candidates":[{session_id, run_id, conversation_id, state, cwd, created, reference}]}`, with at most 20 candidates and the newest first. The candidate lines show the working folder and start time, never transcript content. The desktop and mobile clients never meet this error, because they always address by ID. Not-found messages ("no such session", "session not found", "no saved binding") become one "no session named NAME" message with the same exit code as today.

### 5. Desktop and organization

- **Rows.** Rows already use `run:` keys. Labels that repeat in a visible list get a short suffix (folder or start time) so the person can tell the sessions apart. Identical labels never merge.
- **Drafts and read marks.** Composer drafts and desktop read marks move from computer and name to computer and session ID, with a one-time fallback that reads the name key.
- **Catalog membership.** Decided in P2c-1 (coordinator, 2026-11-04; choice A, revised after the #1413 review). This replaces the earlier `id:<session_id>`-in-`Key::Session` wording.
  - Every writer on the session's own computer dual-writes: the name membership, then its ID twin. `swarm apply` derives the twin for this computer's sessions.
  - A twin names the session ID, its label and the ID of the exact name operation it mirrors. Twins form a last-writer-wins register per session ID, ordered by their own clock, actor and ID, and are never compared with catalog clocks. A twin whose name operation a later one superseded is pruned at the next save, because it can never be read again; a twin of an operation not known here yet stays. A twin's clock counts from the twin this computer last had for its ID, so after a lost or unreadable twins file it starts at 1 again. A peer's older twin may then win until the owner writes again, but it is read only while its own operation is still selected: the loss can hide an ID membership, never show a stale one.
  - Twins live beside the catalog (`swarm/session-id-twins.json`), never in it. hgs v0.47.0 parses `catalog.json` with a closed key enum, so a rollback on the same computer, and v0.47.0 peers through sync, export, preview and join, keep working. No catalog floor is raised. Only members that list the `session_id` kind exchange twins; `swarm export` gives a requester only the kinds it lists, and one that lists none only the kinds v0.47.0 parses.
  - A twin is readable only while the operation it mirrors is still the selected name membership of its label, with the same project. A member that never receives twins keeps Lamport clocks without them, so a comparison of clocks could let its later move tie with or trail a twin. Binding to the operation makes any later move by name, superseding or concurrent, hide the twin until its owner writes it again.
  - `membership_by_id` is ready only when every machine of the catalog is this computer or a direct peer whose last sync listed the kind. Unknown machines fail closed.
  - Desktops read the ID form only when it is ready. The name form is still written, and read everywhere else, until P3 decides the connector's capability and when to stop writing names, together with owner questions 1 and 3.
- **Rename and restore.** Rename no longer refuses a label in use, and restore no longer needs a free name. Fork keeps its readable `-fork`, `-2`… suffixes, but a taken label stops being an error.

## Phasing

| Phase | Change | Label rule |
| --- | --- | --- |
| P0 (done, #1223) | Archived sessions never hold a label; a fork suffixes a taken label | unique among live and saved |
| P1 | `session_id` in records, `HGS_SESSION_ID`, hooks and machine clients resolve by ID, `--session-id` and references on every command, the ambiguity error | still unique, so nothing visible changes |
| P2 | The ID index (P2a), `journal_name` for new sessions (P2b), catalog membership, drafts and read marks by ID, dual-written (P2c). `terminal_name` and the `sessions/<id>` layout for a reused label moved to P3 (coordinator, 2026-11-04). | still unique |
| P3 | Remove the uniqueness checks from launch, rename, restore and fork | labels may repeat |

Each phase ships separately and keeps older CLIs, peers and records working.

Constraints (coordinator, 2026-11-04):

- P1b and every later phase must not depend on cross-version ID continuity. Hooks, `--session-id` and machine clients treat `session_id` as a hint and fall back to the run and conversation identities when an older CLI has changed it.
- P3 must not ship before hooks resolve by `session_id` through P2's O(1) layout. In P1, hooks keep the O(1) name-first lookup and treat `HGS_SESSION_ID` as a hint. The record found by name is taken at once when it carries that ID. Otherwise the run scan prefers the records that carry it, and when none does, the run identity alone decides, as before P1. Name-first is only safe while names are unique.
- P2a (coordinator, 2026-11-04) provides that O(1) index without moving records. `ids/<session_id>.json` names the label and record kind of the live or saved session with that ID. It is a verified cache, never a source of truth: every hit reads that record again and needs that session ID, no archive and no rename shadow. A miss or a stale entry falls back to one scan. `--session-id` resolution repairs the entry under the state writer lock when the lock is free; hooks never repair, so their poll loops never take the lock. A process that finds `ids/` or an entry it must not write, or fails to change one, changes no entries for a minute, then checks again (zerus-2sd29), so a transient failure in a long-lived writer pauses its notes rather than ending them. On Unix, `ids/` must also be owned by the effective user with its owner write bit set, so a foreign-owned directory costs no lock and no write. A broken index therefore costs each lookup one scan, a lock at most once a minute, and never a hook's lock. Archive and forget retire the entry under their lock. An older hgs ignores `ids/`, so a rollback loses nothing. Records stay at their `sha256(label)`, `bindings/` and provider binding paths; `sessions/<id>.json` is used only for a session that reuses a taken label, in P3. `--session-id` resolution and the agent's own hooks (`HGS_SESSION_ID`) read the index.
- P1c must not move a machine client's hot path (polling, status, watch) onto `--session-id` until P2's O(1) index exists. Until then `--session-id` scans the bindings once per explicit call. P3 is gated on P1 and P2 being deployed to every computer in the swarm, and the mobile connector, because an older peer can only report ambiguity, not resolve it.
- P2b (coordinator, 2026-11-04): rollback safety covers v0.47.0 and every later build. A session created by a launch gets `journal_name = session:<session_id>` and `launch_label`.
  - Resume, rename, Start fresh, Clear and restore carry both; recovery only sends input.
  - A `--fresh` launch over a saved, confirmed conversation of the label is a new session with its own journal and `launch_label`. Only a retry of an unconfirmed attempt continues that attempt's journal and label. Legacy name-only tasks also match `launch_label`.
  - Every v0.47.0 path reads and keeps `journal_name`. It drops the unknown `launch_label` on resume, and a later rename here pins the label it leaves.
  - On v0.47.0, an explicit `-r ID` relaunch that replaces a saved P2b record would continue under the label rather than the ID journal. That is the only journal-continuity gap.
  - A fork's lineage keeps its three-part `[journal, conversation, sequence]` entries, which earlier builds read. The labels its sources had go in a separate `fork_lineage_labels` map, carried on resume, so inherited rows still show the source's label. Where the map is missing, the direct source falls back to the `fork_source_name` the fork recorded.
  - Fork rollback round trips:
    - A P2b fork resumed by v0.47.0 loses `fork_lineage`. If its source is later renamed or archived, the derived lineage finds nothing and saves an empty lineage for good, so the inherited view stays empty.
    - A fork resumed by an earlier lineage-era build keeps its lineage but drops the labels. Its direct source then shows its recorded label; older ancestors show their journal name.

P3 checklist, besides the constraints above:

- **Deferred sends** (core-cl, 2026-11-04). The deferred-send actor's pre-spawn check, `current()` in `src/desktop/deferred_send/target.rs`, reads the session record at `sha256(name).json`. When P3 keeps the record of a reused label at `sessions/<session_id>.json`, that check must read it there and pass the session ID to `hgs send`. Otherwise every deferred send settles as never started: safe, but broken.

## Owner questions

1. **Launch with a label in use.** Is the proposed rule right?
   - Attach when exactly one session has the label.
   - Refuse when several do.
   - From P3, `--new` adds another session with the label, so a script that relied on `--new` refusing would then get a second session.

   The alternative is that a launch always creates a new session.

   **Owner answer, 2026-11-04:** "a name must not be an identifier; it is just a name." The resulting rule:
   - The desktop and mobile New session always create a new session, whatever the name; duplicates are allowed. From P3 nothing anywhere refuses a name because another session has it.
   - Every machine client and agent coordination path addresses sessions by session ID or reference, never by name.
   - `hgs NAME` stays only as compatibility for scripts. It opens the one session with that name; with several, it refuses with the ambiguity list (exit 5) and never guesses. From P3, `hgs NAME --new` always creates a new session, even when the name exists.
2. **Terminal names.** Should a new session keep its label as the tmux name when free, which keeps `tmux attach -t LABEL` working? The alternative is always ID-based terminal names: simpler, but a visible change for anyone attaching by hand.

   **Coordinator decision, 2026-11-04** (the owner asked for the release to run without these answers and may override it): keep the label as the tmux name while it is free, so `tmux attach -t LABEL` keeps working; use an ID-based terminal name only when the label is already taken.
3. **Fork names.** Should forks keep the `-fork`/`-2` suffix once labels may repeat (proposed, for readability), or reuse the source label as-is?

   **Owner answer, 2026-11-04:** "the fork dialog already asks for the name." The resulting rule:
   - From P3, the name the person enters is used exactly as entered, even if another session has it.
   - Until P3, the automatic `-2` stays and the dialog keeps its "in use" hint.
   - The pre-filled suggestion stays `<name>-fork` for readability, and a CLI fork without a name keeps `-fork`.
   - The `-2` search goes once names may repeat.
4. **Swarm rollout.** Is one release window of dual-written catalog membership acceptable, or should P2 wait for an explicit catalog version gate?

   **Coordinator decision, 2026-11-04** (the owner may override it): dual-write catalog membership, both the name and `id:<session_id>`. Readers switch to ID membership, and the name form stops being written, only behind an explicit catalog version gate that every computer and the mobile connector advertise. There is no time-based release window.

The owner answered questions 1 and 3 on 2026-11-04, so P3 may be planned after P2c.
