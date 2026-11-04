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
- **Catalog membership.** It moves to `Key::Session { machine, session: "id:<session_id>" }`, the same pattern as today's `archive\n<id>`. Older peers would read an `id:` member as an unknown session and show it in the default project. For one release window the desktop therefore writes both keys and resolves a conflict in favour of the ID key. A swarm drops name keys only once every peer reports a version that reads ID keys.
- **Rename and restore.** Rename no longer refuses a label in use, and restore no longer needs a free name. Fork keeps its readable `-fork`, `-2`… suffixes, but a taken label stops being an error.

## Phasing

| Phase | Change | Label rule |
| --- | --- | --- |
| P0 (done, #1223) | Archived sessions never hold a label; a fork suffixes a taken label | unique among live and saved |
| P1 | `session_id` in records, `HGS_SESSION_ID`, hooks and machine clients resolve by ID, `--session-id` and references on every command, the ambiguity error | still unique, so nothing visible changes |
| P2 | `terminal_name`, the `sessions/<id>` layout with a label index, `journal_name` for new sessions, catalog, drafts and read marks by ID (dual-written) | still unique |
| P3 | Remove the uniqueness checks from launch, rename, restore and fork | labels may repeat |

Each phase ships separately and keeps older CLIs, peers and records working.

Constraints (coordinator, 2026-11-04):

- P1b and every later phase must not depend on cross-version ID continuity. Hooks, `--session-id` and machine clients treat `session_id` as a hint and fall back to the run and conversation identities when an older CLI has changed it.
- P3 must not ship before hooks resolve by `session_id` through P2's O(1) layout. In P1, hooks keep the O(1) name-first lookup and treat `HGS_SESSION_ID` as a hint. The record found by name is taken at once when it carries that ID. Otherwise the run scan prefers the records that carry it, and when none does, the run identity alone decides, as before P1. Name-first is only safe while names are unique.
- P1c must not move a machine client's hot path (polling, status, watch) onto `--session-id` until P2's O(1) index exists. Until then `--session-id` scans the bindings once per explicit call. P3 is gated on P1 and P2 being deployed to every computer in the swarm, and the mobile connector, because an older peer can only report ambiguity, not resolve it.
- P2b (coordinator, 2026-11-04): rollback safety covers v0.47.0 and every later build. A session created by a launch gets `journal_name = session:<session_id>` and `launch_label`.
  - Resume, rename, Start fresh, Clear and restore carry both; recovery only sends input.
  - A `--fresh` launch over a saved, confirmed conversation of the label is a new session with its own journal and `launch_label`. Only a retry of an unconfirmed attempt continues that attempt's journal and label. Legacy name-only tasks also match `launch_label`.
  - Every v0.47.0 path reads and keeps `journal_name`. It drops the unknown `launch_label` on resume, and a later rename here pins the label it leaves.
  - On v0.47.0, an explicit `-r ID` relaunch that replaces a saved P2b record would continue under the label rather than the ID journal. That is the only rollback gap.
  - A fork's lineage keeps its three-part `[journal, conversation, sequence]` entries, which earlier builds read. The labels its sources had go in a separate `fork_lineage_labels` map, carried on resume, so inherited rows still show the source's label. Where the map is missing, the direct source falls back to the `fork_source_name` the fork recorded.
  - Fork rollback round trips:
    - A P2b fork resumed by v0.47.0 loses `fork_lineage`. If its source is later renamed or archived, the derived lineage finds nothing and saves an empty lineage for good, so the inherited view stays empty.
    - A fork resumed by an earlier lineage-era build keeps its lineage but drops the labels. Its direct source then shows its recorded label; older ancestors show their journal name.

## Owner questions

1. **Launch with a label in use.** Is the proposed rule right?
   - Attach when exactly one session has the label.
   - Refuse when several do.
   - From P3, `--new` adds another session with the label, so a script that relied on `--new` refusing would then get a second session.

   The alternative is that a launch always creates a new session.
2. **Terminal names.** Should a new session keep its label as the tmux name when free, which keeps `tmux attach -t LABEL` working? The alternative is always ID-based terminal names: simpler, but a visible change for anyone attaching by hand.
3. **Fork names.** Should forks keep the `-fork`/`-2` suffix once labels may repeat (proposed, for readability), or reuse the source label as-is?
4. **Swarm rollout.** Is one release window of dual-written catalog membership acceptable, or should P2 wait for an explicit catalog version gate?
