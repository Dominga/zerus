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
  - Like `ids/`, it is a verified cache. Every entry is checked against its record: the same session ID and label, no archive and no rename shadow.
  - A miss or a stale entry falls back to one scan of `sessions/`, which stays small because only sessions with a reused label live there.
  - Writers change it under the state writer lock; readers never take that lock.
  - `ids/<session_id>.json` (P2a) gains a record kind for the ID layout, so `--session-id` and hooks reach such a session in O(1).
- **One resolver.** A label's candidates are its legacy holder plus the index's sessions. Every reader that resolves a label goes through this resolver, including `records()`, which also lists `sessions/`.
- **Other files keyed by the label.** For a session in the ID layout, every other file keyed by `sha256(name)` is keyed by the session ID: recovery jobs, the native-input lock and DeepSeek fork intents, and the hash that `hook_health`, `task_reconcile`, `session_graph`, `session_project` and `launch_observation` recompute. Sessions at the legacy path keep their files.
- **Journals.** A reused label always belongs to a new session, which has its own `session:<session_id>` journal since P2b. Journals named by a label before P2b stay with their own session.

### Terminals (question 2, coordinator)

- Records gain `terminal_name`. A new session uses its label when no terminal session has that name, else `label~<first 8 characters of its session ID>`.
- Control goes through `terminal_name` or tmux's `$N`, never the label. That covers `=NAME` targets, `has-session`, `set_run_marker`, kill, titles, viewers and attach, and the live snapshot that matches records to panes.
- A record without `terminal_name`, written by an older build, uses its label, which is then its terminal name.
- A rename moves the terminal name to the new label only when that name is free.
- The Windows host registry keys sessions by terminal name and still refuses a duplicate terminal name. It never sees a label.

### Selection among sessions that share a label

- **Resolving a label:**
  - no candidate: not found, or for a launch, a new session;
  - one candidate: that session;
  - several: the expected IDs select. The `--expected-run-id` and `--expected-conversation-id` flags and the JSON `expected_run_id` and `expected_conversation_id` fields keep the candidates whose run, and conversation if given, match. Exactly one left is that session; none left is each command's stale-target refusal, as today; several left is the ambiguity report (exit 5).
- **Older clients.** The desktop, Android, iOS and the connector send expected IDs with every write. Their builds from before `--session-id` therefore still reach the right one of several sessions. A command sent by bare label without expected IDs, such as pause or kill from an older desktop, fails closed with exit 5.
- **Other selectors.** `--session-id` and references select as in P1. `--all` commands go through records, not labels.
- **Listings.** `hgs ls --json` merges live, saved and archived data by session ID instead of label, so a saved session is hidden only when the same session is live. Watch rows keep their `run:` keys.

### The fleet gate and session emission

- **The gate.** It is open on a computer when every computer it addresses accepts labels that repeat:
  - every machine of its swarm catalog and every configured peer lists a new `session-labels-v1` line in `hgs --help`;
  - its mobile gateway connector reports the same.

  Answers are cached like `session-id-selector-v1` (10 minutes). An unknown or unreachable computer keeps the gate closed.
- **A transition only** (coordinator, 2026-11-05). The gate exists only while a swarm is being upgraded. Nobody turns it on or off: it opens by itself on each computer once every computer it addresses is upgraded, and from then on the owner's rule holds in full.
- **While it is closed,** P3 behaves as P2, and nothing refuses that did not refuse before:
  - a new or forked session that would reuse a label in use takes the next free `-2`, `-3`… suffix, exactly as a P2 fork does;
  - the person sees why. The fork dialog keeps its hint ("That name is in use. The fork will be named %1."), and New session on the desktop and phone shows the same kind of hint with the name it will take;
  - rename, restore and `hgs NAME --new` refuse a label in use, as they do today.
- **The connector's marker** (coordinator, 2026-11-05). The gateway connector advertises `session-labels-v1` only once it resolves sessions and project membership by session ID, reading the catalog's ID twins. Otherwise the phone would merge sessions that share a label into one project. This is part of the connector work that adds the marker, a prerequisite of P3d and P3f, so the gate cannot open while the phone would merge them (`zerus-v8tzi`).
- **Session emission.** `hgs reference`, `hgs self` and sender envelopes add the optional `session` field (the session ID) only while the gate is open, because a parser older than P1c-1 rejects unknown fields.

### P3 checklist

- **Hooks.** Hooks resolve by `HGS_SESSION_ID` through `ids/`, also for sessions in the ID layout. The remaining `read_run` callers move to session ID plus run: supervisor, lifecycle, interrupt, continuations, launches, waits, Kimi, Claude approval, the question terminal and `session_graph` sends.
- **Deferred sends** (core-cl, 2026-11-04). `current()` in `src/desktop/deferred_send/target.rs` reads the session record at `sha256(name).json`. It must read the record through the resolver by session ID and pass `--session-id` to `hgs send`. Otherwise every deferred send to a session in the ID layout settles as never started: safe, but broken.
- **Drafts and read marks.** They are keyed by session ID since P2c-2 (#1418, #1421). Read marks are still delivered with `hgs read NAME`, guarded by run and conversation; that delivery moves to `--session-id` where the computer supports it.
- **The Windows host registry**, by terminal name.
- **tmux terminal names**, per question 2.
- **The ambiguity report.** `session_id::ambiguity` has no caller yet; every label resolution uses it.
- **DeepSeek and OpenCode bindings** follow the ID layout for reused labels; their launch and rename refusals go behind the fleet gate.
- **Catalog membership.** The name form stays written. Sessions that share a label share one name membership, and their ID twins tell them apart once `membership_by_id` is ready. The gateway connector resolves sessions and membership by ID, reading twins, before it advertises `session-labels-v1` (a P3d and P3f prerequisite; see the fleet gate).

### Rollback

| Step | After a rollback of a computer, older builds see | What breaks |
| --- | --- | --- |
| P3a–P3c | Nothing new on disk except `terminal_name` and index entries, which they ignore. Every label is still unique and at its legacy path. | Nothing. A build older than P3b that rewrites a record drops `terminal_name`, which then equals the label anyway. |
| P3d | References with `session` | A build older than P1c-1 cannot parse them. It also lacks the marker, so the gate stays closed until it is updated, and closes again on a rollback. |
| P3e, labels in use more than once | Only the legacy holder of each label. Sessions in the ID layout are invisible to older builds but never deleted: they are not listed as saved, and a live one shows as an untracked terminal `label~<id>`. | Hooks of a session in the ID layout update nothing: v0.47.0's `read_run` takes the label's record only when its run matches, then scans only `sha256` paths. Such a session cannot be resumed until the computer moves forward again. Nothing is deleted, because no older build writes `sessions/` or `labels/`. To use such a session on the older build, archive it first: archives are keyed by ID, and the older build restores it under a free label. P3e states both rules in `docs/reference.md`. |
| P3e, other computers | — | A peer or desktop on an older build that names a label held by several sessions gets exit 5 when it sends no expected IDs: a refusal, never another session. |
| P3f | Older desktops and phones list every session; rows sharing a label are told apart only by folder and time. | Their actions without expected IDs refuse with exit 5. |

### PRs

1. **P3a, readers.**
   - The label resolver, the ID layout and the label index, read but not written.
   - `ids/` entries for the ID layout, and `records()` over `sessions/`.
   - Files keyed by session ID for that layout.
   - Hooks and the `read_run` callers by session ID; the deferred-send check.
   - The ambiguity report wired in, still unreachable.
2. **P3b, terminal names.** `terminal_name` on new records, every tmux and host target through it, and the Windows host registry by terminal name. While labels are unique, every terminal name equals its label.
3. **P3c, selection.** The expected IDs select among candidates, `hgs ls --json` merges by session ID, and `--all` iterates records.
4. **P3d, the fleet gate and emission.** The `session-labels-v1` marker in the CLI, the gate with its cache, and the `session` field in references, `hgs self` and sender envelopes behind it. The gateway connector adds its own marker only with its work to resolve sessions and project membership by ID, which is a prerequisite of opening the gate and of P3f.
5. **P3e, labels that repeat (CLI).** With the gate open:
   - launch `--new`, rename, restore and fork write the ID layout when the label is in use;
   - `hgs NAME` reports ambiguity;
   - fork takes the entered name, and the `-2` search goes;
   - DeepSeek and OpenCode follow;
   - `docs/reference.md` states the rollback rules: sessions in the ID layout are invisible to older builds but never deleted, and to use one on an older build, archive it first.

   With the gate closed, it behaves as P2, with the `-2` suffix and its hint for new sessions and forks.
6. **P3f, desktop and phone.**
   - New session always creates. While the gate is closed it shows the `-2` hint, and once the gate opens it shows none.
   - Once the gate opens, the fork dialog drops its "in use" hint and keeps the `<name>-fork` suggestion.
   - Rows that share a label get a short folder or time suffix.

   Live connector and gateway updates stay owner-gated.

P3e merges only after P3a–P3d are installed on every computer of the swarm and on the gateway connector. The fleet gate then opens by itself, computer by computer.

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
