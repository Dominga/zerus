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
- **Rename, restore and fork.** From P3, rename takes a label in use and restore no longer needs a free label. A fork uses the name the person enters exactly as entered; the dialog suggests `<name>-fork`, a CLI fork without a name uses `<tag>-fork`, and the `-2`, `-3`… search goes (owner, question 3). See the P3 plan.

## Phasing

| Phase | Change | Label rule |
| --- | --- | --- |
| P0 (done, #1223) | Archived sessions never hold a label; a fork suffixes a taken label | unique among live and saved |
| P1 | `session_id` in records, `HGS_SESSION_ID`, hooks and machine clients resolve by ID, `--session-id` and references on every command, the ambiguity error | still unique, so nothing visible changes |
| P2 | The ID index (P2a), `journal_name` for new sessions (P2b), catalog membership, drafts and read marks by ID, dual-written (P2c). `terminal_name` and the `sessions/<id>` layout for a reused label moved to P3 (coordinator, 2026-11-04). | still unique |
| P3 | Readers by label and session ID (P3a), terminal names (P3b), selection among sessions that share a label (P3c), the fleet gate and session emission (P3d), labels that repeat behind that gate (P3e), and the desktop and phone (P3f). See the P3 plan. | labels may repeat once the fleet gate is open |

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

## P3 plan (2026-11-05)

The owner's answers to questions 1 and 3 and the coordinator's decision on question 2 set the target. P3 ships as six PRs. The first four change nothing visible while every label is unique, the fifth lets labels repeat once the fleet gate is open, and the sixth updates the desktop and phone.

### Target behaviour

- New session on the desktop and the phone always creates a new session, whatever its label. Nothing refuses a label because another live or saved session has it.
- `hgs NAME` stays for scripts:
  - it opens the one live or saved session with that label;
  - with several, it refuses with the ambiguity report (exit 5, section 4) and never guesses;
  - with none, it creates one, as today.

  `hgs NAME --new` always creates a new session, even when the label is in use.
- Rename and restore take any label. A fork uses the name the person enters, exactly as entered. The fork dialog suggests `<name>-fork`, a CLI fork without a name uses `<tag>-fork` whether or not it is taken, and the `-2`, `-3`… search goes.
- Machine clients and agent coordination address a session by its session ID or reference, never by its label alone.

### Storage: the first holder keeps the label's path

- **The first holder.** A session whose label no other live or saved session holds is stored as today:
  - `sha256(label).json`, or `bindings/` through `name_routes`;
  - its provider binding stays in `dsh/bindings/` or `opencode/bindings/`.

  Most sessions never move, and older builds keep reading them.
- **The ID layout.** A session created, renamed, restored or forked onto a label that another live or saved session holds is stored at `sessions/<session_id>.json`. Its provider binding goes to `dsh/sessions/` or `opencode/sessions/`. It stays there for life and never moves back, even after its label becomes free.
- **The label index.** `labels/<sha256(label)>.json` (`{version, label, ids}`) lists the sessions stored by ID under a label.
  - It is always a superset (#1430 review, desktop-u G1). The index entry is durable before its record takes the label: it is created there, renamed to it or restored to it. A record write refuses when its index write failed. The record leaves the label, by a rename, an archive or a forget, before its index entry is removed. A durable record therefore always has an index entry, so a verified index is complete for every creation made under the writer lock, and a crash or a concurrent lock-free reader can only see an extra entry, never miss one.
  - Every entry is checked against its record: the same session ID, label and layout, no archive and no rename shadow. A stale entry is dropped by that check and removed by the next writer.
  - **Completeness manifest** (review-q2, decided by the coordinator, 2026-11-05; made per label after desktop-u's #1437 G1). `labels/.complete` lists every label that has a session in the ID layout, keyed by the label's SHA-256, with the SHA-256 of that label's file: `{"version":1,"labels":{<label hash>:<file hash>}}`.
    - **Proof.** It is evidence for one label at a time:
      - a label the manifest does not list has no session in the ID layout;
      - a listed label has exactly the sessions its file lists, and that file must be there, readable and well formed, and hash to its entry.
    - **Writers.** They work under the writer lock, first rebuilding a manifest that is missing or invalid from a bounded enumeration of `sessions/` and the label files. They write a label's file and then its entry. Removing a label's last session takes the entry out first, then the file.
    - **Hand deletion.** The manifest lives inside `labels/`, so a hand deletion of `labels/` removes it. Label resolution by a command, never a hook, rebuilds it when the writer lock is free at once, as P2a repairs `ids/`.
    - **Hot paths.** Bare-label resolution reads the manifest and, for a listed label, one small file; it does not scan. Hooks resolve through `ids/` and supervisors through their selected ID, never through label resolution. Label resolution runs where a command names a label: at the CLI's dispatch of a session command, in the launch and uniqueness checks, and in readers that only have a name.
    - **Native readers.** The task executor proof and the deferred-send target read the same proof through their held state root. When nothing proves the label's sessions, they refuse rather than scan, unless `sessions/` does not exist at all.
  - A missing, changed or unreadable `labels/` file, or a manifest that is missing or does not match it, is never trusted: it counts as incomplete (coordinator, 2026-11-05; #1437 review). So does a present candidate record that is unreadable, replaced, a link or a FIFO, which is never taken as proof of absence. Every read in the ID layout is bounded, never follows a final link and never blocks on a FIFO. None of them ever lets a reader select the label's first holder. The reader then scans `sessions/` off every hot path, and that scan is bounded to 2,000 records or 2 seconds, whichever comes first. If completeness still cannot be proven within that bound, the resolution refuses: exit 5, or the closed answer. It never picks the legacy holder.
  - The final effect admission of every command revalidates the original it selected.
  - Writers change it under the state writer lock. Readers never wait for that lock: only label resolution by a command takes it, and only when it is free at once, to rebuild a manifest. Tests cover a crash between each pair of writes.
  - `ids/<session_id>.json` (P2a) gains a record kind for the ID layout, so `--session-id` and hooks reach such a session in O(1).
- **One resolver.** A label's candidates are its legacy holder plus the index's sessions. Every reader that resolves a label goes through this resolver, including `records()`, which also lists `sessions/`.
- **The rollback-floor guard.** A session in the ID layout never writes a `name_routes` entry, a legacy rename shadow or any other file at a label's path, and `mirror_legacy`, `sync_legacy_locked` and `remove_legacy` skip it. A rename of such a session rewrites only its own `sessions/<id>.json` and the label index; its terminal follows by `$N`. Renaming it away from a label whose first holder remains leaves that holder byte-identical.
- **Readers outside `records()`.** P3a also moves these readers, which scan the binding directories or resolve a label themselves (#1430 review):
  - the binding scanners:
    - `worktree_cleanup::bindings()`, a deletion path, so that an ID-layout session counts as using its checkout;
    - `task_bindings_in`, behind task routing;
    - the search index's catalog and queue scanners, and its record path `sha256(identity.name)` through `name_routes`;
    - `voice.rs`, which reads the DeepSeek and OpenCode bindings;
    - `watch/stamps.rs` `FOLDERS`, which gains `sessions/`, `dsh/sessions/` and `opencode/sessions/` so `hgs watch` notices their changes;
  - the `record_path(name)` callers that resolve a label without the resolver: `read_marks::current` (`hgs read`), `notifications`, `effort`, `terminate`, `untracked_shell`, `session_action`, `recovery`, `journal`, `watch_source`, `attempts`, `lifecycle`, `session_project`, `archive` and `desktop/maintenance/io.rs`.

  A P3a test enumerates the binding-directory scanners: it searches the source for reads of the binding directories and fails on any that does not also cover the ID layout, so a new scanner cannot miss it.
- **Other files keyed by the label.** For a session in the ID layout, every other file keyed by `sha256(name)` is keyed by the session ID: recovery jobs, the native-input lock and DeepSeek fork intents, and the hash that `hook_health`, `task_reconcile`, `session_graph`, `session_project` and `launch_observation` recompute. Sessions at the legacy path keep their files.
- **Existing sessions that move.** A rename or a restore can move an existing session from its legacy path into the ID layout, when its new label is held by another session. A moved session keeps everything it had:
  - its `journal_name`, run and conversation, history, drafts and read marks;
  - its outstanding request identities, recovery job, fork intent and receipts.

  New sessions have their own `session:<session_id>` journal since P2b. Journals named by a label before P2b stay with the session that has them, including one that moves.

### Terminals (question 2, coordinator)

- Records gain `terminal_name`. A new session uses its label when no terminal session has that name, else `label~<first 8 characters of its session ID>`.
- Control goes through `terminal_name` or tmux's `$N`, never the label. That covers `=NAME` targets, `has-session`, `set_run_marker`, kill, titles, viewers and attach, and the live snapshot that matches records to panes.
- A record without `terminal_name`, written by an older build, uses its label, which is then its terminal name.
- A rename moves the terminal name to the new label only when that name is free.
- The Windows host registry keys sessions by terminal name and still refuses a duplicate terminal name. It never sees a label.
- **As built in P3b** (coordinator, 2026-11-05: D1–D7 with conditions):
  - **Minting.** A launch mints its terminal: the label, else `label~<id8>`, else `label~<session ID>`, else it refuses (the 8-character suffix collision). It never attaches to or reuses an occupied name, and a name taken before the launch makes the launch fail.
  - **When a suffix appears.** While labels are unique, only a resume or Start fresh of a saved session whose label an untracked tmux session holds gets a suffix. A terminal counts as untracked when its environment has no `HGS_SESSION`; a terminal some hgs launch started, the session's own resume still starting included, keeps a resume away as already live. A new launch, fork, rename and restore keep refusing a label that any tmux session holds.
  - **The supervisor.** The launch passes `HGS_TERMINAL_NAME`. The supervisor records `terminal_name` from its pane's session name and keeps `HGS_SESSION` as the label when the two agree. Otherwise it keeps the earlier adoption of the pane's name as the label, but never adopts a terminal that another record claims. It selects a session in the ID layout under the label.
  - **Routing.** `terminal_name` only routes. Every native action still verifies the pane, run marker and process it verified before; a mismatch refuses and never falls back to the label's terminal. A stale `terminal_name` whose terminal is gone, or now another's, shows the session as not live.
  - **Rename.** A rename moves the terminal by its `$N` under the writer lock and records the new name. Its transaction keeps the terminal's name before the move for recovery; a failed move changes nothing. A rename that moves no terminal, of a stopped or paused session, clears `terminal_name` (#1445 review).
  - **Liveness** (#1445 review). A session is live only when its own pane and run match, or while a launch of its own label (`HGS_SESSION`) holds its terminal or any name a launch of it could mint. A terminal another session's launch started, or one hgs never started, never makes it live and never hides its listed row. The environment read reports present, absent or unavailable, and an unavailable read refuses at every check before a launch; a suffix needs proven absence.
  - **Listings.** Live rows come from the terminals and are listed under the record's label; their pane folders follow.
  - **Archives.** An archive and a restore drop `terminal_name`.

### Selection among sessions that share a label

- **Resolving a label:**
  - no candidate: not found, or for a launch, a new session;
  - one candidate: that session;
  - several: the expected IDs select. The `--expected-run-id` and `--expected-conversation-id` flags and the JSON `expected_run_id` and `expected_conversation_id` fields keep the candidates whose run, and conversation if given, match. Exactly one left is that session; none left is each command's stale-target refusal, as today; several left is the ambiguity report (exit 5).
- **`hgs read` and other guards with other field names.** Read marks are delivered as `hgs read NAME` with `run_id` and `conversation_id` in its JSON (#1430 review). P3c selects with those guards too, and with `--session-id` where the computer supports it. Without that, an ID-layout session would get exit 5 forever: the desktop would show it read while its host never does, #1421's failure mode. P3c names every command whose guards use other field names. Read delivery selecting this way is a P3e precondition.
- **Older clients.** The desktop, Android, iOS and the connector send expected IDs with every write. Their builds from before `--session-id` therefore still reach the right one of several sessions. A command sent by bare label without expected IDs, such as pause or kill from an older desktop, fails closed with exit 5.
- **Other selectors.** `--session-id` and references select as in P1. `--all` commands go through records, not labels.
- **Listings.** `hgs ls --json` merges live, saved and archived data by session ID instead of label, so a saved session is hidden only when the same session is live. Watch rows keep their `run:` keys.

### Native continuity rules (desktop-u G1, coordinator 2026-11-05)

- **Layout is necessary, not authority.** A record's `layout: "id"` and the existence of `sessions/<id>.json` are both needed, and neither authorises anything. Every read, write, rename, archive, forget and hook verifies the original session ID, label, layout and run under its actual read or commit scope.
  - Paths come only from fixed canonical IDs: never a caller-supplied path, and never a fallback to the label's holder.
  - The existing process, pane and birth checks, and the frontend, Service, configuration, grant and lease checks, stay as they are.
  - Indexes route; they grant nothing.
- **One exclusion domain across a move.** A rename or restore that moves an existing session into the ID layout keeps one exclusion domain across the move: the same input lock, recovery job and fork intent identity. While an owned worker still holds the old lock, the move refuses and stops no agent for it; the native-input driver held across `effort::apply_settings` is one such worker. A rename during a possible delivery leaves that delivery Unknown, never a reissued job.
  - **Dormant Unix preparation.** The native-input transition guard takes the original legacy lock once, without waiting, and adds the fixed session-ID key as a hard link to that same held object. It pins the private directories, original record, both names and its original deadline; a queued driver that already opened the old name still contends on the same kernel lock. Interruption retains both names for explicit recovery. This substrate has no production caller, metadata move or writer activation; committed-record recovery, the permanent negative reservation below, other per-session jobs and the Windows method remain prerequisites.
  - **Permanent old-key reservation** (coordinator decision, 2026-11-08). Once a legacy session moves, its old `hash.lock` and `SID.lock` names retain the same original inode permanently, without unlink, relock or reconstruction. No route or shadow remains at the label path. A new session under that label uses its own new ID layout and input key whenever the legacy key is reserved; unknown, irregular or unreadable reservation evidence also selects the ID layout, never reuse of that legacy key. An existing first legacy holder stays untouched. A new ID-layout session never creates a legacy alias, so retained alias pairs and old-key negative entries are bounded by the number of legacy sessions ever moved. Per-SID transition metadata remains bounded and is never pruned by age. Reservation survives rename, archive, restore and recovery of both the moved session and later sessions under its label. Recovery from a crash after the record move but before reservation publication remains fail-closed. Required controls cover a later label's distinct new-SID exclusion, an old keep-warm worker holding the pre-move name key, archive/restore, and that crash cut. Alias deletion is no longer an activation prerequisite: any future physical retirement needs a separate proposal, positive retirement proof and review; elapsed time, `ps` or idleness never suffices.
    Preparation reserves the original legacy key before publishing its SID alias. A single immutable entry of at most 4 KiB binds the exact original record digest, SID, lock object and state/input/reservation directories; a fixed original-namespace anchor prevents a rebuilt empty directory from establishing absence. Interrupted preparation retains its one negative entry too, never another entry per retry or later label holder. Missing or changed originals refuse recovery without recreating either name. These dormant APIs establish exclusion and negative allocation only: the complete record/job transaction, its per-target recovery and constructor callers remain prerequisites.
  - **Exact-SID input kind** (coordinator decision, 2026-11-08). The same anchored `native-input-reservations` scope also retains one immutable, at most 4096-byte versioned kind file per SID. Its exact `sid-<canonical UUID>.json` name cannot collide with the old 64-hex-key filenames; no listing or derived index discovers it. The old-key v1 schema and loader remain compatible. A `Fresh` kind and a NOREPLACE SID lock precede the new terminal ID record publication; no legacy alias is minted. A `LegacyMoved` kind exactly matches the original v1 reservation's SID, old key, before-image digest and lock identity and precedes moved publication without creating a new lock. Native acquisition opens existing originals only and requires the matching kind. Neither an absent transition marker nor a singly linked SID lock proves Fresh. Missing, malformed, future, mismatched or unreadable kind evidence refuses with no fallback or repair; a moved record that loses its marker never becomes Fresh. SID entries number one per ID publication identity, like the SID lock itself, including retained interrupted preparations with no record. Orphans are never reused, deleted or pruned by age. These APIs and the new-identity publisher remain dormant with no constructor/runtime activation; the complete native writer floor remains false.
  - **Finite worker rounds on Unix.** Recovery and keep-warm hold separate pinned, nonblocking round locks across their observation, submission and result writes. A future metadata transaction may try those same locks while it holds the state writer; it never waits for a round that may need that writer. The lifetime recovery-worker ownership lock is not that exclusion. Keep-warm reads its setting snapshot again after acquiring its round, and a setting choice rechecks its original binding under the settings lock before writing. These advisory zero-byte locks accept an owner-controlled legacy readable directory without changing its mode; they never establish the stronger private-root proof required by native journals and ID publication. The metadata/job move, exact original request/history carry, recovery and permanent old-key reservation remain prerequisites; these locks alone activate no writer.
- **Dormant picker guard, Cut A** (successor coordinator decision, 2026-11-08). The inactive Unix facade borrows the actual Root writer, retains the original private root/session/input/receipt directories, bounded record and exact SID kind proof, and takes the matching existing-only Fresh or LegacyMoved kernel exclusion. Neither a legacy label nor missing evidence supplies a fallback. Each literal command, named key and Escape callback has its own fresh native-check callback followed by an original-scope/deadline check; the possible-send flag is set immediately before that callback can deliver. Record and receipt commits have separate fresh barriers and receive the bounded current record and pinned destination, rather than an admission snapshot or a new pathname. A successful original record commit may advance its retained record descriptor only with the same exact target binding. Refusal is sticky, retains exclusion until the facade is dropped, and invokes no later cleanup key or stale commit. Nested work borrows the same facade and original deadline. The moved exclusion accepts an exact empty conversation as correlation only; first-message readiness and native settings checks are unchanged.
  These are trusted Rust callbacks with no production caller, backend delivery, receipt reservation/result classification or new permission. Existing immutable request, consent, native identity/pane/panel and receipt checks remain required at later integration. The proposed 15-second active-operation budget is a logical dormant-test budget, not a hard helper-return promise: Root-lock waiting and synchronous helper drain remain separate. Live Cut B is not admitted; the original-helper/cancellation/physical-drain prerequisite is tracked as `zerus-ucck.4.9`, and runtime integration as `zerus-ucck.4.10`. No runner or process-group change is part of Cut A. Writers and `native_continuity` remain false; first runtime wiring requires separate source G1, qualification and the coordinator's Arch/Mac preview.
  **Dormant in-process helper placement.** The owned variant moves the same actual Root lock into the scope; the borrowed Cut A remains unchanged. Before the trusted guarded spawn, the existing bounded helper-custody primitive retains that scope's original directories, record, typed SID proof and kernel input exclusion. Each possibly effectful spawn follows the original/native barrier and records possible delivery before the helper can act through argv. Empty stdin EOF has a separate fresh barrier and grants no native authority. Once a helper exists, the trusted spawn must return that same Child, including post-spawn refusals, without detaching pipes or reaping it elsewhere. Cancellation, expiry, post-attach unwind and caller Drop retain the original scope through the existing retirement poller until the original wait, stdin closure/worker join and both real output EOFs/joins settle. Lost wait evidence never becomes settlement or a reconstructed PID. Only an actual physical report can return the owned scope; helper failure and changed originals keep its native effects and final commits refused. This is dormant in-process custody, not a hard bound on the trusted callbacks or preceding writer acquisition, custody after the CLI/host process exits, cross-process crash recovery, a delivery receipt, replay permission or live Cut B admission. A future per-command CLI caller therefore still needs its original host/process-lifetime or inherited-custody placement. No controller, helper runner, process-group policy or writer gate is changed.
- **Preserved legacy Git veto** (coordinator decision, 2026-11-08). A normal Git-created `0644` worktree lock is an advisory veto, not a private-file authority proof. Dormant metadata-move preparation uses a separate version-2 `legacy-advisory-veto` proof; the existing version-1 private guard remains unchanged. It retains the same original regular, singly linked, current-user-owned object, with owner read/write access, no group or other write access, and no setuid, setgid, sticky or special file type. Every component is opened without following symlinks. The exact original mode and file version, `zerus:SID` reason, launch ownership nonce, checkout, common directory and Git metadata objects stay bound under the state-writer-to-repository try-lock order. This proof establishes no native private-root, process, cleanup or file permission. Preparation never creates, chmods, relocks, unlinks or rewrites the Git veto. It first persists the negative protection marker in the original legacy binding, then publishes the bounded proof without replacement. Missing, foreign, changed, writable or hardlinked locks, manual unlock or same-text relock, and lost marker/proof all refuse and retain the binding for review; they never reconstruct protection. Real-Git controls cover these refusals and the genuine `0644` positive. The record/job transaction and its per-target recovery remain prerequisites; this constructor has no runtime caller or writer activation. Shared retirement of a veto of this new kind remains refused pending its own original-object handoff integration.
- **Dormant terminal record/job transaction.** The Unix mechanism verifies the actual original state writer and takes the native-input exclusion before any pending-record marker. It retains that same input descriptor, both finite worker rounds, the settings exclusion and any original Git veto through publication. Its one bounded per-SID journal keeps the complete original record, exact recovery job/runtime bytes, original request IDs, attempts, histories and deadlines, one typed keep-warm entry and the original native namespace/exclusion identities. Recovery reads that SID only; it never scans all journals. The old job files and legacy record are retained as their original native objects in the private journal, without unlink or replacement. A new SID copy precedes retirement of its old key, the label-index superset precedes the ID record, and the negative record marker stays until an exact completion proof is durable. Settings carry compares only the old and SID keys and preserves later unrelated choices; it never restores a whole stale settings map. Missing, replaced or irregular original exclusions, namespace changes, conflicting before-images and incomplete native-alias preparation refuse without reconstructing them. A completed lookup never recreates a subsequently archived record. The per-record pending predicate has no I/O for records without its marker, so one damaged journal cannot establish a global input fence.
  Resume and Start fresh retain the opaque `layout_transition` value when they rebuild the same session, including null or malformed evidence. Their final writer check binds both its presence and content, so a change during the selected-helper probe refuses before terminal creation. Rename and archive retain it with the full record; archive restore retains it through bootstrap while keeping the existing legacy-layout restore recipe. A new session or fork never inherits another session's marker. Carrying this evidence neither validates completion nor grants input permission.
  A dormant Unix acquisition helper reads only that marked SID's bounded completion and immutable before-image. It checks their hash, original native root/transaction/session namespace and writer object, and the current ID binding before recovering the original protected record's retained old-hash/SID input inode pair. It makes one nonblocking attempt and never creates, relinks or repairs a missing name or proof. Its returned exclusion retains the original proof descriptors and rechecks them, both aliases, the current target binding and original deadline; a failed recheck does not physically release the held inode. Same-SID rename/fresh can acquire a new guard, while an already-held guard refuses a changed target. Another SID with the same label uses its own exclusion. This is correlation and exclusion only, with no runtime driver caller or native operation/permission/receipt acceptance. The ordinary create-capable native-input path is unchanged and still requires its separate integration before writers can activate.
  This remains mechanism-only source with no runtime constructor or recovery caller. The predicate is not yet a wired frontend/actor admission guard. Native-provider and routed/shadow binding carry, ID-layout restore integration, all native writer/reader fences, the selected-helper and original-live-writer floor, and shared retirement for the new Git proof kind still require integration and review. Neither these controls nor a completion file enable duplicate-label writers or `native_continuity`.
- **No reinterpretation.** Nothing falls back to the name key of an ID twin. An offered actor plan is never rewritten to add `--session-id`: an offered or Unknown original keeps its lookup-only settlement. The held-root resolver rule covers every native actor's target readers, `deferred_answer` included.
- **Selected helper continuity.** The private `--run` reader accepts a required count `sid1:<session ID>:<run ID>:N` only when both canonical identities match the tracked launch's declared-new or saved-ID environment. It normalizes only that count before the existing lifecycle. An older helper's integer-only parser refuses this form before bootstrap; the literal floor control must show a bounded non-panic refusal with no state, lock, terminal or native-process change. New tracked terminal launches and forks on Unix emit it only after a positive observation of the selected helper's `session-launch-identity-v1` marker or verified equality with the actual running code that implements that parser. Positive and negative answers are cached per exact helper image inside the launching process (64 paths; a busy/full cache keeps legacy). A timeout, overflow, unreadable answer or image replacement during the probe keeps the integer/legacy recipe, clears the new-ID declaration and permits no ID-layout publication or shared-label writer. A new CLI process has its own cache; the normal same-image path needs no help child. On Unix, saved resume and Start fresh of an existing ID-layout record require that same positive cached helper answer and emit the required count with the existing session ID and the new run. A missing or uncertain capability refuses before terminal creation and names the computer whose hgs must be updated: an integer recipe could let an older helper overwrite a different legacy holder with the same label and native conversation. The final state writer rechecks the session/run/conversation, layout, label and recorded terminal against the prepared recipe. Legacy saved sessions, archive restores and dry descriptions keep their integer recipes without a capability probe; Windows retains its existing recipe until its continuity method is qualified. The required count also protects against a helper replacement after the capability check.
- **Exact terminals.** A collision of the 8-character suffix, or an occupied terminal name, is never a reason to attach to or reuse that terminal. Native targets stay the exact terminal, pane and run that were checked.

### The fleet gate and session emission

- **The gate.** It is open on a computer when every computer it addresses accepts labels that repeat:
  - every machine of its swarm catalog and every configured peer lists a new `session-labels-v1` line in `hgs --help`;
  - its mobile gateway connector reports the same.

  Answers are cached like `session-id-selector-v1` (10 minutes). Any member's hello, or a change in a member's reported version, drops the cached answer at once, so a computer or connector that downgrades closes the gate at its next sync rather than after 10 minutes. An unknown or unreachable computer keeps the gate closed, and so does a machine that left the swarm but is still in its catalog, until it is removed from the catalog.
- **A transition only** (coordinator, 2026-11-05). The gate exists only while a swarm is being upgraded. Nobody turns it on or off: it opens by itself on each computer once every computer it addresses is upgraded, and from then on the owner's rule holds in full.
- **Not authority.** The gate and its cache are transition mechanisms and authorise nothing (desktop-u G1).
  - Observations are bound to the verified machine and its configured route. They are invalidated on a replacement, a rollback, or a change of membership or connector, and an unknown answer closes the gate.
  - The receiving CLI enforces the compatible floor itself, and all selectors a command supplies must agree.
  - A retry for an unsupported selector never strips selectors after a possible attempt.
  - The gate covers every answering route and the actual supported connector, until ID session routing and ID membership routing both work there. The declared inventory below defines supported connectors.
  - Live deploys stay owner-gated.
- **While it is closed,** P3 behaves as P2, and nothing refuses that did not refuse before:
  - a new or forked session that would reuse a label in use takes the next free `-2`, `-3`… suffix, exactly as a P2 fork does;
  - the person sees why. The fork dialog keeps its hint ("That name is in use. The fork will be named %1."), and New session on the desktop and phone shows the same kind of hint with the name it will take;
  - rename, restore and `hgs NAME --new` refuse a label in use, as they do today.
- **The connector's marker** (coordinator, 2026-11-05). The gateway connector advertises `session-labels-v1` only once it resolves sessions and project membership by session ID, reading the catalog's primary per-ID membership and its compatible ID view. Otherwise the phone would merge sessions that share a label into one project. This is part of the connector work that adds the marker, a prerequisite of P3d and P3f, so the gate cannot open while the phone would merge them (`zerus-v8tzi`).
- **Declared connector inventory** (coordinator, 2026-11-07). Supported connectors are those managed by the maintained installers, with `--config` equal to `core::mobile::config_path()`: Linux's user unit is `zerus-mobile-connector.service`; other platforms include an equivalent only where their installer creates one. A canonical configuration or installer unit closes the gate until the actual journal-owning connector answers a fresh bounded, data-only nonce query positively, bound to its generation, configuration and machine. The installed executable's help is not evidence of the running instance. An old or absent responder keeps the gate closed. No canonical configuration and no installer unit means no supported connector on that node. Hand-started connectors with custom `--config` paths and legacy services outside the installer are outside this supported boundary: stop or upgrade them before labels repeat. Do not discover them by executable names or build a general process inventory. Live connector deployment remains owner-approved.
- **Session emission.** `hgs reference`, `hgs self` and sender envelopes add the optional `session` field (the session ID) only while the gate is open, because a parser older than P1c-1 rejects unknown fields.

**P3d implementation.** The fixed read-only `swarm session-labels-context` answer carries a required `labels_by_id` bit as well as ready ID membership. The foundation build's missing bit is not a positive, even if it shares the new build's package version. The running connector's nonce reply advertises its own ID routing only after its auxiliary reads and native project assignment resolve through the exact listed run's ID and primary membership. Its positive reply also requires the runtime routing markers to be current; a failed help reprobe cannot claim readiness. Its observation loop checks every 200 ms, retaining the original two-second query and one-second native-read budgets. This does not change API v1, durable command identities or actor recovery forms.

Ordinary/fence hello can report a proven absent connector immediately; it never waits on a connector. The existing swarm sync worker separately makes bounded local and peer observations. The cache has at most 257 slots (local plus 256 configured peers), with an atomic attempt and answer file for each. A new hello retires its old answer before reading the scope or making a connection. Completion only answers that original nonce; a late reply cannot restore an older pointer. The hot gate is read-only and never connects or starts a worker. It checks the ten-minute age, current configuration, verified route, catalog roster, primary availability and declared connector inputs again. Unverified routes, an unaddressed catalog machine, an old/unknown reply, or unavailable data closes it. A damaged primary does not silently become an empty one. Interim twins-only peers therefore cannot open this full-label gate, independently of the older membership-ready projection (`zerus-1nkva`).

Fresh connector requests capture one native-listed session ID for their exact run. The same selection stays across parent/child inspection and other auxiliary reads, and the existing peer envelope accepts it only on those closed command forms. Background attention reads select each listed run separately. Archive calls, original offered actor plans and status/recovery calls are not rewritten; an unsupported selector is never stripped after a possible attempt. Project assignment sends the same selected ID, validates its native echo, and keeps it out of the unchanged phone reply schema. Exact primary null/conflict is unassigned, not permission to borrow a twin's label membership. Parent and human-request fields pass through unchanged.

P3d enables capability observation and conditional reference emission, not duplicate-label writers. Those remain P3e, followed by P3f presentation. The canonical live connector is upgraded only after the owner approves its deployment.


### The Qt GUI

The Qt GUI (`tray/src`) is part of neither the gate nor P3f. It addresses sessions by name through the CLI. With labels that repeat, a command it sends for a shared label fails with exit 5, and it shows that error: never another session. This is accepted, because the Qt GUI is the rollback GUI (coordinator, 2026-11-05).

### P3 checklist

- **Hooks.** Hooks resolve by `HGS_SESSION_ID` through `ids/`, also for sessions in the ID layout. The remaining `read_run` callers move to session ID plus run: supervisor, lifecycle, interrupt, continuations, launches, waits, Kimi, Claude approval, the question terminal and `session_graph` sends.
- **Deferred sends** (core-cl, 2026-11-04). `current()` in `src/desktop/deferred_send/target.rs` reads the session record at `sha256(name).json`. It must read the record through the resolver by session ID and pass `--session-id` to `hgs send`. Otherwise every deferred send to a session in the ID layout settles as never started: safe, but broken.
- **Drafts and read marks.** They are keyed by session ID since P2c-2 (#1418, #1421). Read marks are still delivered with `hgs read NAME`, guarded by run and conversation; that delivery moves to `--session-id` where the computer supports it.
- **The Windows host registry**, by terminal name.
- **tmux terminal names**, per question 2.
- **The ambiguity report.** `session_id::ambiguity` has no caller yet; every label resolution uses it.
- **DeepSeek and OpenCode bindings** follow the ID layout for reused labels; their launch and rename refusals go behind the fleet gate.
- **Catalog membership.** The name form and derived twins keep their existing schemas. The separate primary per-ID causal membership graph keeps sessions with the same label independent. Valid twins are promoted before a shared-name write; exact ID data (including null or a reviewable conflict) wins, with name fallback only when no ID record exists. Only peers advertising `session_id_memberships_v1` exchange that graph. Older name-only writers cannot edit ID grouping. The gateway connector resolves sessions and membership by ID before advertising `session-labels-v1` (see the fleet gate).

### Primary ID project membership (coordinator, 2026-11-07)

The name membership and its clock-bound ID twins remain byte-compatible; the derived twins do not silently become primary data. A separate, bounded primary membership record addresses `(machine UUID, session_id)` and is exchanged only under a new advertised catalog capability. Older builds keep their name/twin readers and writers and preserve this separate record, but cannot edit the new ID grouping. New readers prefer the exact ID record and fall back to the name membership only when no ID record exists; unavailable or malformed evidence is not absence.

Project membership currently uses `src/swarm/catalog.rs`'s causal operation graph, not a document in `crates/replica`. Reuse that existing graph and membership resolution for the separate ID map. Preserve every raw causal head: one known live concrete project wins over concurrent Inbox/null defaults; distinct concurrent concrete projects remain reviewable; a later explicit Inbox or unassignment consumes all observed heads and wins. Do not introduce last-writer-wins per ID. Migrate valid existing twins under the catalog lock before any shared-name change, idempotently, without overwriting an existing ID record. Tests cover those four merge cases and two same-label IDs in different projects surviving both rename and peer exchange. This metadata grants no native authority and adds no general inventory.

### Rollback

The complete rollback floor for labels that repeat contains P3a, P3b and P3c plus the native-continuity corrections found before activation (desktop-u G1, coordinator 2026-11-05 and 2026-11-08):
- the resolver and the in-place handling of the ID layout (P3a);
- `terminal_name` routing for every native target, including rename's `Target::Name`, tmux `=NAME` targets and the Windows host registry (P3b);
- exact selection by expected IDs (P3c);
- releasing an owned `zerus:` worktree lock on archive and forget, so that no lock lingers after a rollback to the floor (P3c; nothing creates such a lock before P3e, coordinator 2026-11-05).

The native-continuity corrections include fixed ID-layout DeepSeek/OpenCode bindings, launch liveness by `HGS_SESSION_ID`, and pending DeepSeek forks keyed by the source session ID. Two same-label sources with different native conversations retain separate pending request UUIDs; an explicit retry retains its own original destination and UUID through a rename, and a changed host leaves the unknown request for manual review. Reader/reference-only P3d builds do not establish that complete floor.

The outer CLI session-label observation/cache must carry a narrow required `native_continuity` boolean before activation. P3e writers need a fresh positive from every CLI; a missing, false or unknown value keeps writers closed. This is separate from the connector Context schema and adds no native consent or general inventory. P3d reference emission can use its existing reader gate. Canonical connector deployment remains separately owner-approved; no resident bridge is reloaded for the transition.

They stay separate PRs. A computer rolled back from P3e to that floor or later keeps every session that shares a label working, and never writes one to `sha256(label)`, where it would overwrite that label's first holder. Any later native dispatcher, helper or Service actor must also sit at that floor. Only a rollback below the floor after P3e is the "invisible but never deleted" case in the table.

| Step | After a rollback of a computer, older builds see | What breaks |
| --- | --- | --- |
| P3a–P3c | Nothing new on disk except `terminal_name` and index entries, which they ignore. Every label is still unique and at its legacy path. | A terminal name can already differ from its label in P3b: it is `label~<id8>` when another terminal, such as an untracked one, holds the label. A build below P3b then finds no pane for that session (v0.47.0's `matches` needs the pane and its run marker) and shows it as not live. It never acts on the other terminal, but its `hgs LABEL` attaches to that terminal. A build below P3b that rewrites the record drops `terminal_name`. |
| P3d | References with `session` | A build older than P1c-1 cannot parse them. It also lacks the marker, so the gate stays closed until it is updated, and closes again on a rollback. |
| P3e, labels in use more than once, rolled back below the floor | Only the legacy holder of each label. Sessions in the ID layout are invisible to older builds but never deleted: they are not listed as saved, and a live one shows as an untracked terminal `label~<id>`. | Hooks of a session in the ID layout update nothing: v0.47.0's `read_run` takes the label's record only when its run matches, then scans only `sha256` paths. Such a session cannot be resumed until the computer moves forward again. Nothing is deleted, because no older build writes `sessions/` or `labels/`. To use such a session on the older build, archive it first: an explicit archive by its exact ID, on a compatible build, before the rollback. Archives are keyed by ID, and the older build restores the session under a free label. Nothing archives automatically, terminates an agent, or archives an invisible twin after the rollback. The older build's worktree maintenance scans only the four binding directories, so the checkout of an ID-layout session would look unowned to it; P3e protects it with a Git worktree lock (see the worktree lock). P3e states these rules in `docs/reference.md`. |
| P3e, other computers | — | A peer or desktop on an older build that names a label held by several sessions, without expected IDs, gets exit 5 from the receiving CLI, which is ambiguity-aware because it is at or above the floor: a refusal, never another session. A computer below the floor sees only each label's first holder and reports no ambiguity. |
| P3f | Older desktops and phones list every session; rows sharing a label are told apart only by folder and time. | Their actions without expected IDs refuse with exit 5. |

### PRs

1. **P3a, readers (with P3b and P3c, the rollback floor).**
   - The label resolver, and sessions in the ID layout read, written and renamed in place, archived and forgotten, but never created. A write keeps such a record in place only when the record says `layout: "id"` and `sessions/<id>.json` exists, and it refuses when the two disagree. Every read path verifies the same session ID and label, no archive and no rename shadow, including hooks through `ids/`.
   - Resume and Start fresh keep the session in place. An archive drops `layout`, so a restore on the floor writes the label's own path and needs a free label, as today. A launch that replaces a session in the ID layout without keeping its ID, a retry of an unconfirmed attempt or a new session over a stopped one, writes the label's path and then removes the replaced record, so no stale twin stays; a failed removal leaves both, never neither.
   - `ids/` entries for the ID layout, and `records()` over `sessions/`.
   - Files keyed by session ID for that layout.
   - Hooks and the `read_run` callers by session ID; the deferred-send check.
   - The readers outside `records()` listed under storage, and the test that enumerates binding-directory scanners.
   - The rollback-floor guard: no routes, shadows, mirrors or removals at label paths for ID-layout sessions; renames in place; the label index kept a superset, with crash-window tests.
   - The ambiguity report wired in, still unreachable.
2. **P3b, terminal names.** `terminal_name` on new records, every tmux and host target through it, and the Windows host registry by terminal name. While labels are unique, every terminal name equals its label.
3. **P3c, selection.** The expected IDs select among candidates, including `hgs read`'s `run_id`/`conversation_id` guards and every other command whose guards use other field names, and `--session-id` where the computer supports it. `hgs ls --json` merges by session ID, and `--all` iterates records. P3c also releases an owned `zerus:` worktree lock on archive and forget (see the worktree lock, item 6).
4. **P3d, the fleet gate and emission.** The `session-labels-v1` marker in the CLI, the gate with its cache, and the `session` field in references, `hgs self` and sender envelopes behind it. The gateway connector adds its own marker only with its work to resolve sessions and project membership by ID, which is a prerequisite of opening the gate and of P3f.
5. **P3e, labels that repeat (CLI).** With the gate open:
   - launch `--new`, rename, restore and fork write the ID layout when the label is in use;
   - `hgs NAME` reports ambiguity;
   - fork takes the entered name, and the `-2` search goes;
   - DeepSeek and OpenCode follow;
   - the checkout of an ID-layout session is protected by the worktree lock, under its rules below;
   - `docs/reference.md` states the rollback rules: sessions in the ID layout are invisible to older builds but never deleted; to use one on an older build, archive it first; and its checkout stays locked until it is archived or forgotten.

   Preconditions: read delivery selects among sessions that share a label (P3c), and the complete native-continuity floor is installed on every computer.

   With the gate closed, it behaves as P2, with the `-2` suffix and its hint for new sessions and forks.
6. **P3f, desktop and phone.**
   - New session always creates. While the gate is closed it shows the `-2` hint, and once the gate opens it shows none.
   - Once the gate opens, the fork dialog drops its "in use" hint and keeps the `<name>-fork` suggestion.
   - Rows that share a label get a short folder or time suffix.

   Live connector and gateway updates stay owner-gated.

P3e merges only after the complete native-continuity rollback floor for labels that repeat is installed on every computer, and P3d on every computer of the swarm and on the gateway connector. The fleet gate then opens by itself, computer by computer.

### The worktree lock (P3e; desktop-u and review-q2, coordinator 2026-11-05)

The lock keeps worktree maintenance below the floor from removing the checkout of an ID-layout session; it is a veto for that maintenance only.
1. **Only owned worktrees.** P3e locks only a linked worktree whose verified Zerus ownership receipt (`worktree_lifecycle::owned`) matches the session's `worktree_owner`. It never locks a custom or existing worktree the person chose, and it runs no Git operation on a plain folder that is not in Git. When the person already holds their own Git lock on such an owned checkout, the move into the ID layout refuses with a clear reason and the session stays where it is. The person's lock is their choice: Zerus neither takes it over nor treats it as its own (coordinator, 2026-11-05).
2. **Lock before publication.** The lock is acquired and durably verified before the canonical record is published in, or moved into, the ID layout. A lock that fails, is uncertain or is interrupted never leaves an ID-layout record exposed. A protective lock left over by an interrupted publication stays for review, with no cleanup by age or name.
3. **Verify at every mutation.** The ownership nonce, object and common-dir binding, and the current lock, are verified at every mutation. A foreign or user lock is never overridden, and `--force` is never used. The exact `zerus:<session_id>` text is necessary for a release, but it is not by itself ownership or permission to remove anything.
4. **Shared checkouts.** A release considers all live or saved sessions that use the same checkout: forks share the checkout and its `worktree_owner`. Archiving or forgetting one must not remove another's protection. The reason is transferred under the writer and exclusion scope without a gap, or the unsupported shared case refuses. One session per worktree is never assumed, and protection stays when the evidence is unavailable.
5. **A veto, not consent.** The lock never consents to a deletion. A confirmed removal keeps every existing check: ownership, source, files, other occupants, processes, drain and fingerprint. Releasing the lock never authorises deleting a branch or data, stopping an agent or archiving automatically.
6. **Release at the floor, and the person's choices.**
   - The floor honours `zerus:` locks, and P3c releases them on archive and forget, so none lingers after a rollback to the floor. That release applies rules 3 and 4: it verifies ownership and the current lock, never forces, and keeps the protection while another live or saved session uses the checkout.
   - Plain `git worktree remove`, `move` and `prune` refuse a locked checkout, and a checkout deleted by hand keeps its branch pinned until it is unlocked; `docs/reference.md` says so.
   - Zerus does not lock a checkout again after the person unlocks it: the person chose.
   - **As built in P3c** (coordinator, 2026-11-05: L1–L4 with conditions):
     - **Trigger.** The release runs under the writer lock when a session leaves. A session leaves by `archive::save` (manual archive, terminate, the supervisor and reconcile), by `terminate --forget`, or by forgetting its archive. Only the exact text `zerus:<session_id>`, with or without Git's newline, is a candidate.
     - **Ownership.** The record's `worktree_owner` must match `worktree_lifecycle::owned` by nonce and metadata entry.
     - **Other sessions.** `worktree_cleanup::bindings` is the strict, bounded scan of live and saved sessions: by folder, and now by ownership ID. Since this change it reads a session in the ID layout from its own record, never its label's holder. Any of these keeps the lock: another user, an unreadable binding or more than 5,000 bindings.
     - **Take.** A durable intent in `worktree-locks/` comes first. The lock is then renamed to `locked.zerus-release-<nonce>` beside it. Its text is checked exactly, and only then is it removed. Any failure after the take puts it back with a hard link that never replaces a lock that appeared meanwhile. If one did appear, a taken lock with the exact text goes and any other stays for review.
     - **Recovery.** It settles each intent right after the writer lock is taken, and never fails that lock.
     - **Reporting.** A kept lock is reported on standard error and never fails the archive or forget.
     - **The shared case.** A lock whose session left while another still uses the checkout keeps that session's text. Transferring the reason is P3e's.

P3e's controls for the lock:
- a lock that fails or is interrupted before publication;
- a foreign or replaced lock, and a replaced checkout;
- two ID-layout sessions sharing a checkout, with one archived;
- the exact release by the owned last holder;
- v0.47.0's maintenance, run literally, refusing the locked checkout.

The P3e publication foundation is dormant: no production creator calls it and
it does not open the duplicate-label writer gate. Its create-only publisher
requires the caller's writer/input exclusion and a preallocated canonical ID;
an existing identity is never recreated or overwritten. A bounded, verified
label-index superset and the original owned checkout's Git veto precede the
canonical ID record. An interrupted publication keeps its veto for review.

The protection-receipt directory is held without following aliases, and its
native object is part of the receipt. Reads and atomic writes use that held
directory. Losing the receipt cannot downgrade an ID-layout record to the legacy
text-only release rule. Before a shared transfer to a legacy holder, the exact
census-bound record receives a durable negative `worktree_protection` marker;
its provider exclusion is try-only, and resume/fresh rebuilds carry the marker.
Missing or inconsistent proof keeps the lock for review. The marker grants no
publication, removal or signal authority.

A private receipt binds that veto to the ownership nonce and the native state,
checkout, common-directory, metadata and lock objects. A missing Ready lock
means an unlock or interruption, never permission to relock. Shared retirement
transfers the fixed-length reason through the same original file descriptor,
under the writer and nonblocking repository exclusion, without an unlocked gap.
Unknown holder identities keep the veto. Last-holder release checks the original
file again after taking it; the existing recovery puts an unfinished take back.
An explicit person's unlock is respected, and only a confirmed Zerus release
permits a later new publication. None of this is removal or signal consent.

Initial terminal creation and fork allocate one session UUID and run UUID before
the native fork/terminal spawn. A separate internal declaration binds the public
`HGS_SESSION_ID`/`HGS_RUN_ID` handoff; the current helper consumes it before native
help, refuses a mismatched final record, and never adopts a caller's inherited
public session ID as a child's allocation. Resume keeps its saved ID and clears
the new-launch declaration. This allocation alone does not publish the ID layout.

Native constructor/transition continuity and the required outer CLI writer bit
remain prerequisites before P3e activation; the canonical connector deployment
is still owner-approved separately.

The outer CLI observation also carries a typed `native_continuity` capability,
separate from the connector-facing Context schema. Reader-only two-field answers
remain valid for readers; writer admission requires an explicit `true` from the
same fresh answer of every enrolled CLI and all existing cache/configuration/
connector checks. Missing, false, mistyped, stale or incomplete evidence stays
closed. This build advertises `false`, and its dormant public writer gate stays
closed even for a simulated positive cache. The bit can become positive only
after the complete reviewed storage/native floor; it is never native consent.

### Acceptance controls (desktop-u G1)

Each PR lists which of these it covers, and every one is covered by P3e at the latest:
- a valid but incomplete index, and a cut at every publication and retirement step; the leaf PRs prove the superset and completeness rule with them (desktop-u, standing condition);
- a present candidate that is unreadable or replaced, which is never proof of absence, and a bounded or unknown result, which never selects the first holder;
- an ID record that is corrupt, missing or replaced, and duplicate session IDs;
- two writers choosing one label at once;
- a rename into an occupied label while the original's input lock or recovery job is held;
- a collision of the 8-character terminal suffix;
- stale selectors, and unqualified actions from older clients;
- below-floor readers and hooks, run literally as a build below the floor runs them;
- a rollback to the floor with two live or saved sessions that share a label, proving that their terminals stay apart and that writes stay in place.

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
