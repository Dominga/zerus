# Project and team memory: RustCodeLore in Zerus

Status: specification (task 122, owner request of 2026-11-07; amended by the
owner's answers to its seven questions, task 141, 2026-11-07). Nothing here is
implemented yet. RustCodeLore is the Rust implementation of CodeLore
(`D:/Tools/CodeLore`, Python 0.3.0, github.com/WhiteWh/CodeLore): it starts as a
port in `crates/lore` here and becomes the implementation itself (section 2.1).
The Python version stays the conformance reference until the Rust one matches
it, then it is retired (section 8).

## 0. The owner's decisions and where this document answers them

### 0.1 The seven answers (2026-11-07)

| # | The owner's answer | Design |
|---|---|---|
| 1 | Memory gets several behaviour modes, like a team's plan modes standard and free — not a Planning-like choice. | A team's memory mode: `plain`, `team`, `project`, `read-only` or `off`, chosen at creation and switched while it runs like task 58's plan mode, owner-confirmed (sections 4.1 and 4.2). |
| 2 | A store under Zerus data is its own private git repository: yes. | Section 2.5: `git init` only in Zerus's own store folder, `plain` commits, no remote. |
| 3 | The areas are a hierarchy: General (descriptions and shared quick facts across the project's folders), then one area per project folder, then one per team created under the project (under the project, not under a folder). | Section 4.4: the project store's area tree with `general` at its root, `folder.<slug>` areas and `team.<slug>` areas under the project; section 3.1 creates them. |
| 4 | A finished team's workspace is kept and searchable; it is exported into the project store only by a project rule or the owner's request. | Section 4.5: kept read-only and searchable; the project's `export_finished` rule or the owner's Export; never automatically otherwise. |
| 5 | Faults are written automatically when every field is known. | Section 6.1: a fault is written as soon as symptom, cause, fix and check are known; only a record with a missing required field becomes a lead inbox item. |
| 6 | RustCodeLore fully replaces the Python CodeLore: CodeLore is rewritten in Rust; the code lives in `crates/lore` here and moves into github.com/WhiteWh/CodeLore as the new implementation; Python is the conformance reference until parity, then retired. | Sections 2.1 and 8: the crate is the working implementation now, becomes CodeLore 1.0 in its own repository, and the Python 0.3.0 tree is retired after the conformance points hold. |
| 7 | The home-computer model is a flag: by default writes wait for the home computer; the owner can ask for synchronisation between computers instead. | Sections 3.4-3.6: `sync = false` (the default, home computer) and `sync = true` (Loro replicas in `crates/replica`), with the conflict rules of section 3.5 and the owner-confirmed switch of section 3.6. |

### 0.2 The earlier points (2026-11-07, task 122)

| The owner's point (2026-11-07) | Design |
|---|---|
| (1) The user decides whether a project uses project memory. When a project is declared in Zerus (a project may group several folders or repositories), RustCodeLore can be turned on for the whole project, and the user chooses where its database lives: by default under Zerus's own data folder, or another folder. | One store per catalog project. The project's memory setting lives in the project's replicated settings, so every computer of the swarm sees it. The default path is under the state root; another folder can be chosen, and the store can be moved with a confirmation and no data loss (section 3). |
| (2) When a team is created and its project has RustCodeLore on, the team gets a shared workspace in it by default. | A team workspace is a sub-store inside the project store. The team writes there; in the `project` mode reads span the workspace and the project store (section 4.3). |
| (3) When the project has it off, New team offers the memory mode like the behaviour mode: plain memory or RustCodeLore, RustCodeLore by default. | New team picks the team's initial memory mode (section 4.1): `project` when the project's store is on, else `team`; `plain`, `off` and `read-only` where allowed. Memory is a mode of the running team, not a Planning-like creation choice (answer 1). |

## 1. What exists today

### 1.1 CodeLore (Python 0.3.0)

The facts the port must keep. References are to `D:/Tools/CodeLore`, HEAD
`dbe3ceb`.

- **Format:** version 3 (`docs/FORMAT.md`).
  - Records are canonical JSON lines: sorted keys, compact separators, UTF-8, LF line ends, no BOM, sorted by id, a final newline.
  - `lore.toml` holds the configuration. Records live in `lore/<area>.jsonl`, plans in `lore/plans/<plan-id>.jsonl` (the plan, then its items) and sources in `lore/sources.jsonl`.
  - A reader refuses any file that breaks the rules, naming the file and line.
- **Kinds:** fault, reading, decision, topic, recipe, plan, item and source. Each has required fields, character limits and statuses (`schema.py:42-125`). Item moves are restricted (`ITEM_MOVES`).
- **Strictness:** `validate()` refuses and never repairs. Every write goes through one path (`ops.apply`), and a refusal writes nothing.
  - **References:** a reference a change adds is checked: commits and files against git, and records, superseded/reversed links, deps (with no cycles) and plans against the store.
  - **Duplicates:** these are refused (shared signal, cosine ≥ 0.95, or Jaccard ≥ 0.5 over title/main/cause), unless `--new` is given.
- **Git:** `lore init` refuses outside a git repository. Commit modes:
  - `none`: working tree only;
  - `plain` (the default): `git commit --only` of the changed files;
  - `shared-tree`: a commit built in a temporary index, with a compare-and-swap of the branch and three tries.

  The subject is `lore: <verb> <kind> <id>`, with a body line per record and trailers.
- **No writer lock:** concurrency is the SQLite busy timeout (120 s, WAL), the shared-tree compare-and-swap, and an atomic replace with retries.
- **Index:** disposable SQLite under the user cache. That is `$LORE_CACHE`, else `%LOCALAPPDATA%\codelore` (Windows), `~/Library/Caches/codelore` (macOS) or `$XDG_CACHE_HOME`/`~/.cache` + `/codelore`, then `/<sha1(root)[..16]>/index.sqlite`.
  - It is refreshed on open by size, mtime and content hash.
  - It holds FTS5 `words` (porter) and `terms` (code tokens), plus optional vectors.
- **Search:** hybrid, fused by RRF (k = 60, depth 50).
  - The lists are BM25 words, BM25 terms and vectors (Ollama `nomic-embed-text` at `127.0.0.1:11434`, optional).
  - Phrase and signal bonuses apply.
  - Retired records are hidden by default.
- **CLI:**
  - Commands: `lore init|guide|areas|check|fmt|index|find|for|show|exists|cover|add|update|plans|items|stale|plan …|item …`.
  - Common options: `-C`, `--json`, `--by`, `--trailer`, `--mode`, `--text-only`.
  - Every `--json` shape is fixed, and an error is `{"error","details"}` with exit 1.
- **Not present:** privacy rules, teams, workspaces and multi-folder projects. There is one store per repository.
- **Tests:** 68 `unittest` tests in six files.
- **RESULTS.md:** a retrieval benchmark.
  - Corpus: 37 hand-written fault records among 1,490 converted rows.
  - Questions: 82 (an exact and a paraphrase for each of 41 faults).
  - Metrics: answer first, top 3, top 10, MRR, bytes read and wall time per call.
  - lore: 98% / 100% / 100% exact, 71% / 83% / 95% paraphrase (MRR 0.78), 0.75 s a call.

### 1.2 Zerus

- **Projects:** the swarm catalog (`src/swarm`, `config/swarm/catalog.json`). A project has a name and a colour and groups folders on any machine and sessions. It has no per-project settings of its own.
  - The replicated task tracker has per-project `Settings` (`crates/replica/src/project.rs`), synced across the swarm. They are the only per-project settings every computer shares.
  - Project iron rules are local (`<state>/iron-rules.sqlite3`).
- **Teams:** `Team.project` and `Team.project_id` name the project. A team lives in the `teams.sqlite3` of the computer that runs it; members elsewhere hold no team record.
- **What teams record:**
  - tasks with status, decider, reason and history;
  - attempts with reports, commits, checks and runs;
  - findings (severity, file, scenario, state);
  - proposals and their decisions;
  - phase acceptance and rejection with comments;
  - test queue records with per-test results (task 88);
  - merges;
  - about 150 journal event kinds.
- **What agents read:**
  - the lead's brief (`dispatcher.rs brief_text`);
  - each hand-out (`hand_off_text_rules`, with blocks for rules, role, protocol, task and note);
  - the lead's inbox (`inbox::text`);
  - guides (`hgs guide`);
  - the SessionStart hook's `additionalContext` (iron rules, after a compaction too: `hooks.rs start_context`).
- **Private data:**
  - `src/private_stores.rs` checks known private stores and repairs them only inside the state or config root.
  - On Windows a private store is owned by this user, has no access for others but SYSTEM and Administrators, is no reparse point and has one link.
  - On Unix: 0700 directories and 0600 files.
- **New team** has no "behaviour mode". Its mode-like choices are Planning (lead or template), Folder policy and Later-phase approval. Memory is a team mode of its own (section 4.1): New team picks the starting mode, and the running team switches it later like its plan mode (section 4.2).

## 2. (a) The port

### 2.1 Shape

- **Crate:** a new `crates/lore` library, so Zerus links it without a subprocess. It has the same modules as the Python package:

  | Module | Role |
  |---|---|
  | `config` | `lore.toml`, unknown keys refused |
  | `schema` | kinds, fields, limits, statuses, `validate` |
  | `store` | canonical JSONL, order, placement |
  | `gitio` | git plumbing and the three commit modes |
  | `ops` | the one write path, references, relations, ids, commit subjects |
  | `dedupe` | duplicates |
  | `index` | SQLite FTS5 and vectors |
  | `embed` | `ollama` or `none` |
  | `search` | `find`, `for` |
  | `plans` | counts, layers, readiness, stale |
  | `cover` | sources |
  | `guide` | the agent guide text, embedded |

- **The crate is the implementation (answer 6).** `crates/lore` starts as this
  repository's working tree and is carried by Zerus as a path dependency. When
  the conformance points of section 8 hold it moves, whole, into
  github.com/WhiteWh/CodeLore as the new implementation, versioned 1.0, with
  its own CI, releases and guide; Zerus then depends on it by git revision or
  crates.io instead of the path, and the two trees are not kept in sync by
  hand. The Python 0.3.0 tree stays there as the conformance reference and the
  fallback until that move, and is retired after it (section 8). The name
  RustCodeLore is the crate's name during the port and disappears at 1.0,
  where the tool is CodeLore again.
- **Binaries:**
  - `lore`, a standalone CLI with the Python CLI's commands, flags, exit codes and `--json` shapes, byte-identical for the same store. Section 8 holds it to that. It is the same binary in the CodeLore repository after the move, so the owner's scripts and agents see one tool, not two.
  - `hgs lore …`, the same commands addressed by project instead of folder: `--project ID` (or the session's project), and `--team ID` for a team workspace. It also takes `@alias` (section 3.4).
- **Rules carried over:**
  - Error texts and notes stay word for word, so agents trained on CodeLore's guide read the same messages.
  - The format stays version 3. RustCodeLore reads versions 1 to 3 and writes 3. A newer version is refused by name, as in Python.

### 2.2 Records, validation, layout

These are unchanged from FORMAT.md version 3:
- the kinds and fields of section 1.1, with every limit, including those found only in code (reading `value` ≤ 200, `tolerance` ≤ 200, `measured_at` ≤ 64, `by` and `decided_by` ≤ 120, item `commits` elements ≤ 64, plan `doc` ≤ 300);
- the item moves;
- the duplicate rules and thresholds;
- the canonical serialization (Rust `serde_json` with sorted keys and compact separators; tests compare bytes with Python's `json.dumps(sort_keys=True, separators=(",",":"), ensure_ascii=False)`);
- the store's file layout.

Unknown fields are kept and round-tripped.

### 2.3 The writer lock (new)

CodeLore has none: concurrent writers rely on git's compare-and-swap. In the home model one store serves several sessions and computers, so RustCodeLore adds one (a synced store takes it locally per computer; section 3.5):
- **The lock:** an exclusive OS file lock on `<store>/.lore.lock` (`fs2`, as Zerus's build folders use).
- **Scope:** it is held from reading the files a write changes to the atomic replace and the commit. Reads never take it.
- **Contention:** a writer waits up to 30 s, then refuses with "another writer holds the store (`<who>` since `<time>`); nothing was written". The lock file records the holder's process, session and time for that message.
- **Atomic replace:** CodeLore's `<file>.lore-tmp` plus replace, with its Windows retries.
- **`shared-tree` mode** keeps its compare-and-swap for a store inside a git working tree shared with other writers that are not Zerus.

### 2.4 References and multi-folder projects

CodeLore checks `refs.commits` and `refs.files` against the one repository that holds the store. A Zerus project groups several folders, and a store under Zerus data is in none of them. So:
- **The reference roots** of a project store are the project's folders on the computer that holds the store (the catalog's folders with that machine).
- A commit reference must exist in one of them (one batched `git cat-file --batch-check` per root).
- A file reference must exist at HEAD of one of them, as a path relative to that root. The format does not change. A path found in two roots is accepted; the check says which root it took.
- A store inside a project repository (a chosen folder in a repository) checks that repository first, then the other roots.
- A root that is not a git repository checks files on disk and cannot verify commits. A commit reference there is refused with "no project folder on `<machine>` is a git repository that has `<hash>`".

### 2.5 Commit behaviour

**A store under Zerus data (the default) is its own private git repository** (the owner confirmed this, answer 2). Zerus runs `git init` in the store folder when the store is created. This is only Zerus's own store folder under the state root; no project folder is ever initialized. Each write commits as CodeLore's `plain` mode does: the same subjects, bodies and trailers. So history, blame and recovery work as in CodeLore, without touching any project repository.
- `commit.mode` is `plain`, with no remote and no push. Moving the store keeps its history (section 3.3).
- The commit author is Zerus's `by`: the owner, or `agent:<model>` for a session. A trailer `Zerus-Session: <name>` names the session, and `Zerus-Team: <id>` the team when one wrote.

**A store in a chosen folder:**
- **Inside a git repository:** CodeLore's behaviour exactly, `plain` by default. Its `lore.toml` may say `shared-tree` or `none`. Zerus never pushes unless `lore.toml` says `push = true`.
- **Outside any repository:** Zerus initializes the store folder itself as its own repository, as under Zerus data. It does so only after the owner chose that folder and confirmed it, and never in a folder that is inside another working tree.

A store whose git is unusable (no git on that computer, a broken repository) is read-only, with a note naming why. Writes are refused, never written uncommitted.

### 2.6 The index

- **Location:** under the user cache as in CodeLore, keyed by the store's real path. On Windows `%LOCALAPPDATA%\codelore\<key>\index.sqlite`, so the Python and Rust tools can share the folder.
- **Rust's own index:** RustCodeLore writes `index-rs.sqlite` beside it. Each index is disposable and versioned by `user_version`; one tool never reads the other's. Once the Python tool is retired (section 8) only `index-rs.sqlite` is written, and the Python one is removed by the same upgrade. Sharing one index while both exist is not attempted.
- **Inside Zerus:** `LORE_CACHE` is set to `<state>/lore-cache` so the index stays with Zerus's data. It is rebuilt on demand as before.

### 2.7 Out of scope at first

- `cover` (sources and their coverage). The commands are ported but not used by Teams in the first version.
- Push from Zerus-held stores.
- Any embedding provider other than Ollama, and any network model service.
- A web or desktop editor for records. The desktop lists and opens them read-only at first.
- Sharing a store between projects.

## 3. (b) Storage

### 3.1 One store per project

**A project's memory settings** live in the replicated project settings beside `task_template` (the `crates/replica` project `Settings`), so every computer of the swarm sees them:

```text
lore: { enabled: bool, home: <machine id>, path: <absolute path on home, or "">,
        sync: bool, export_finished: bool }
```

- **`enabled`** turns the project's store on (answer 1 of the earlier points).
- **`sync`** chooses the multi-computer model (answer 7): `false` (the default) is the home-computer model of section 3.4, `true` is synchronisation between computers (section 3.5).
- **`export_finished`** is the project rule of answer 4: when true, a finished team's records are exported into the team's area at finish (section 4.5); the default is false, so only the owner's request exports.
- **The earlier point 2** is the default team mode: a new team is `project` when the store is on, so it gets the shared workspace by default (section 4.1).

- **Turning it on** (Projects > the project > Memory) is the owner's action on the desktop, in Qt's ProjectsDialog and Zerus Next's Projects screen.
  - **Home computer:** the owner chooses the computer that holds the store, this one by default. It is the canonical copy in both models and the git history's author in the sync model (section 3.5).
  - **Store folder:** Zerus's default, or another folder on that computer.
  - Zerus then creates the store: `lore.toml` with the area hierarchy of section 4.4 (`general` first, then one area per catalog folder of the project, then the team areas added as teams are created), `git init` (section 2.5), and the privacy checks (section 3.2).
- **The default path** is `<state root>/lore/<sha256(project_id)[..16]>/` on the home computer. `<state root>` is `HGS_STATE_DIR`, else `~/.local/state/hgs/sessions`. A project's id never changes, so renaming a project keeps its store.
- **Turning it off** keeps the store and its files, and stops Teams writing and reading it. Turning it on again finds the same store. Every team whose mode is `project` or `read-only` falls back to its next allowed mode (section 4.7) and the lead is told once.

### 3.2 Privacy

A store may hold code facts and decisions from private repositories, so it is a **private store**:
- `private_stores::known()` lists `lore/` and every store folder under it.
- The same checks apply: owner-only on Windows (no reparse point, one link), 0700 and 0600 on Unix.
- Repair happens only inside the state root, as today. A chosen folder outside the roots is verified, never repaired. If it is not private, the store is refused and listed in `private-stores.json` with the reason. The desktop shows it, and the owner fixes it or chooses another folder.
- The index folder (`<state>/lore-cache`) is private too.
- Nothing from a store leaves the computers that hold it: in the home model only `hgs @home lore …` answers reach the owner's other computers (section 3.4); in the sync model the store travels only through the project's own replicas over the swarm transport (section 3.5), and a project that is not the owner's never receives it.

### 3.3 Choosing and moving the store

- **Choosing** another folder at turn-on: the folder must be empty or already hold a `lore.toml` store of format 1 to 3, which is then adopted as it is.
- **Moving** an existing store (Memory > Move):
  1. The owner picks the new folder; Zerus says what will happen.
  2. A confirmation names both paths, the record count and that the old folder is kept. Cancel is the default and is focused.
  3. Zerus copies the whole folder with its git history, under the writer lock, so nobody writes meanwhile.
  4. It checks the copy: same record count, `lore check` clean, the same git HEAD, and the same bytes for every JSONL file.
  5. Only then does it switch `lore.path`.
  6. The old folder is renamed `<name>.moved-<date>` and kept until the owner removes it from the Memory panel. Zerus never deletes it on its own.
- A move that fails at any step leaves the old store in use and the copy in place, marked incomplete, and says which step failed. Nothing is lost.
- **Moving to another computer** (changing `home`) is the same copy over `hgs @alias`, with the same checks. In a synced project (section 3.5) the `home` computer is the canonical exporter, not the only holder of the store: moving it copies the canonical history to the new home, and the project's replicas keep the records they already have.

### 3.4 The home-computer model (the default, `sync = false`)

This is answer 7's default: a store on one home computer, and writes wait for it.
- The store lives on its `home` computer only. Stores are never replicated: one writer lock, one history.
- **On the home computer:** `hgs lore` opens the store directly.
- **On any other computer:** `hgs lore …` with `--project` runs `hgs @home lore …` over the existing SSH transport (`swarm::remote_hgs`). Every computer of the swarm knows `home` from the replicated settings.
- **Requests:** every write is one request with a request id, so a retried write applies once. That needs a small `requests` table in the store's private folder, as Zerus's other write paths have. Reads are plain.
- **An unreachable home:**
  - Reads say "the project memory lives on `<home>`, which is not reachable".
  - Writes from Teams wait in the team's journal as `lore_pending` and are sent when `home` answers.
  - A person's own `hgs lore add` is refused with the reason.
- **Members' sessions** reach the store the same way, through their own computer's hgs. They need nothing installed beyond hgs.

### 3.5 Synchronisation between computers (`sync = true`)

The owner can ask for synchronisation instead (answer 7). Zerus already has the machinery: the Loro replica in `crates/replica` replicates the project settings, the task tracker and proposals across the swarm. The sync model adds the memory to it.
- **The replica:** every computer of the project that runs the store keeps a local copy under `<state>/lore-replicas/<sha256(project_id)[..16]>/`, a private store (section 3.2). The copy holds the project store and its team workspaces; it is opened by the local `hgs lore`, so reads and writes are local and never wait for a computer.
- **The documents:** each area file is a Loro document in that replica, not a JSONL file to merge. The record set is append-only (a record id is never edited in place; supersede/reverse are relations), so concurrent appends merge as the union of records. `lore fmt` writes the canonical JSONL of the merged replica for export, git history and conformance; the JSONL in a synced store is derived, not edited by hand.
- **Writes:** one write appends the record to the local replica and commits the canonical file locally (section 2.5); the Loro update travels over the existing swarm transport (the SSH/relay path `hgs @alias` uses). Every write keeps its request id, so a retried update applies once. A computer that is offline writes into its replica and syncs when it returns; there is no `lore_pending` wait as in section 3.4.
- **The home computer** stays for the canonical history, the index and the owner's exports: it merges first among the computers that answer, and its `git` history is the published one. A store is still one store with one address (`home`), but writes no longer depend on reaching it.
- **Conflict rules:**
  - **Records** are identified by `(kind, id)`; concurrent creations with different ids merge as a union. The same `(kind, id)` created on two computers with different bytes is a conflict: the record whose own timestamp is later wins (the kind's field: `at`, or `measured_at` for a reading), ties by the lexicographically smaller canonical JSON, and the losing copy is kept under `conflicts/<kind>-<id>.jsonl` and shown to the owner as one Waiting item ("two computers wrote record `<id>`; keep A or B"). Until the owner decides, both copies stay readable with a `conflict` marker, and search says so.
  - **Relations** (superseded, reversed, deps) merge as sets. One record superseded by two different records keeps both relations; the UI shows "superseded twice" and the owner may resolve. A cycle introduced by a merge is refused at merge time and becomes a conflict item, never a half-applied edge.
  - **The project settings** (`enabled`, `home`, `path`, `sync`, `export_finished`) are Loro registers with last-writer-wins: the owner's latest change wins, ties resolve by the replica's Lamport order and then the computer id, and the losing value stays in the Memory panel's history. A change that turns `sync` off while others write applies after the replicas have merged (section 3.6).
  - **Duplicates** (section 1.1) are checked in the local replica at write time and again after a merge; a duplicate found only at merge is a conflict item (keep both, skip, or supersede the older), never a silent drop.
  - **Concurrency:** no cross-computer writer lock. The section 2.3 lock is local to each computer's replica; Loro orders the changes. The `shared-tree` compare-and-swap is not used in a synced store, because the JSONL file is not the writer's file.
- **Switching a store into sync:** the owner's action (section 3.6) copies the store, with its git history, into each participating computer's replica (the same checks as the move, section 3.3), then turns `sync` on. A computer that was never in the project joins by receiving a replica snapshot.
- **Privacy:** the replica folder is a private store; its Loro snapshots are as private as the store. Nothing from it leaves the project's computers.

### 3.6 Choosing the model (owner-confirmed)

- **The switch:** Projects > the project > Memory > "Synchronise between computers" (off by default), in Qt and Zerus Next. Turning it on or off asks first, naming the current model, the computers that will hold replicas and what happens to writes already waiting.
- **Off to on:** the home store is copied (under the writer lock) into the replicas of the participating computers, checked as in the move, then `sync = true` is written; writes that were waiting for `home` in team journals (`lore_pending`, section 3.4) are delivered into the replica once and cleared.
- **On to off:** `sync = false` is written only after the home replica has merged every change (a bounded wait with a status note); writes made meanwhile are kept, and any still-unmerged change is reported rather than dropped. The home computer becomes the only writer again and section 3.4 applies from then on.
- **What does not change:** the store's path, its history and its privacy; a team's memory mode (section 4.1) is independent of the model, and a synced project's teams read and write the local replica.

## 4. (c) The memory modes

### 4.1 The five modes

Every team has one memory mode (answer 1). It decides what the team writes, what its lead and members read, and whether a store is involved. The mode is a team setting beside `plan_mode`, not a Planning-like creation choice.

| Mode | Writes | Reads | Available |
|---|---|---|---|
| `plain` | the team's own record in `teams.sqlite3`, its tests and journal, and the generated digest (section 5) | its own record; the digest in the lead's inbox after a compaction and in `hgs team status --digest` | always |
| `off` | the team's own record and journal only; no digest, no lore | its own record; no digest | always |
| `team` | the team's workspace only: `teams/<team-id>/` in the project store when the project has one, else the standalone team store of section 4.6 | the workspace only: its plans, items, decisions, faults and the records its agents add; never the project store's other areas | always; the workspace is standalone when the project has no store |
| `project` | the team's workspace (as `team`) | the workspace and the project store's areas together; a result says which one holds it | only when the project's store is enabled |
| `read-only` | nothing: Teams' automatic records are skipped (the lead is told once) and an agent's `hgs lore add` is refused with the project's reason; the lead may draft a record for the owner | the project store and, if the team has one, its workspace | only when the project's store is enabled; also allowed on a kept standalone workspace |

- **The starting mode:** New team offers the available modes and preselects `project` when the project's store is enabled (the earlier point 2) and `team` when it is not (the earlier point 3). `plain` and `off` are always offered; `read-only` is offered for a team that should observe memory without writing it.
- **What a store is:** `team`, `project` and `read-only` use a workspace; `plain` and `off` use none. A team's mode does not change the project's `enabled`; it only selects among what the project allows (section 4.7).
- **The workspace is the same one across modes:** the team keeps one workspace per team id. Switching `team`/`project`/`read-only` back and forth attaches the same records; no mode copies or splits them.
- **Plain and off:** plain memory is defined exactly in section 5; `off` is the same team record without the generated digest, for a team whose owner wants no summarised memory at all. Neither writes outside `teams.sqlite3` and its journal.

### 4.2 Switching while the team runs

The owner's answer makes the mode switchable like task 58's plan-mode switch, and always with the owner's confirmation.

- **The owner's switch:** Team > Memory > the mode list in Qt and Zerus Next, or Projects > the project > Memory for a team there. The confirmation names the current and the new mode, which writes stop or start, and what happens to the workspace (kept on disk, attached or read-only).
- **The lead's proposal:** the lead may ask for a mode with a reason (`switch-memory-mode`, the same shape as `switch-plan-mode`): the team stores `memory_mode_offer` and `memory_mode_reason`, the owner confirms or denies it, and a denial is recorded with the reason. Nothing changes without the owner.
- **When it applies:** the mode is written in the same team update as the confirmation; reads use it at once; a write already in flight completes under the old mode and is not re-routed. The next dispatcher tick uses the new mode.
- **What it records:** one journal event `memory_mode_changed` with the old and new mode and the owner's reason; when a store is involved, one `decision` record in the team's area (question "memory mode", choice the new mode, reason, decided_by the owner).
- **What it does not do:** it never creates a new team, never changes the plan mode, never deletes a workspace, and never moves records between stores. Switching to `team` or `project` from `plain`/`off` creates the team's workspace if none exists (or adopts the standalone one); switching to `plain`/`off`/`read-only` keeps the workspace read-only, searchable and exportable (section 4.5).
- **The only rule on the offer:** a mode the project does not allow is not offered (`project`/`read-only` without an enabled store). If the project's store is turned off while a team is in one of those modes, section 4.7's fallback applies with the same notice.

### 4.3 A workspace inside the project store

- **What it is:** a sub-store at `<store>/teams/<team-id>/`, with its own `lore.toml` (`dir = "."`) and the same JSONL layout. It is committed in the store's repository, so one history covers the project and its teams.
- **Created** with the team when the project has memory on, which is the default (point 2). The New team form shows the starting mode and its workspace.
- **Writes:** the mode allows them, as section 4.1 says: Teams' automatic records (section 6) and its agents' `hgs lore add` go to the workspace. The workspace carries the project's area tree, so a record goes to the area its subject names (a folder's area, or `general`) and the team's own automatic records go to its `team.<slug>` area; section 4.5's export copies them into the project store's `team.<slug>`.
- **Reads:** in `project` mode `find`, `for`, `show` and `exists` read the workspace and the project store together; in `team`/`read-only` they read the workspace (and in `read-only`, the project). A result says which one holds it.
- **Ids:** ids are unique across the workspace and the project store. A new record whose id exists in the project store is refused with CodeLore's "already exists" message.

### 4.4 The areas of a project store (answer 3)

The store's areas are a hierarchy, not one flat list:
- **`general`** is the root: the project's description, its cross-folder conventions and the shared quick facts every folder's work relies on. It is created first and holds no folder-specific records.
- **One area per project folder:** `folder.<slug>`, one for every folder in the project's catalog entry, with the folder's label as its description. A record that names a file or commit in that folder goes here by default.
- **One area per team created under the project:** `team.<slug>`, created when the team is created (not when it ends), named from the team's name and keyed by its id. A team area is **under the project, never under a folder**, because a team works across the project's folders; a team's exported records go here (section 4.5), and its area stays as the team's place in the project after the team ends.
- **In the format:** areas are CodeLore's `lore/<area>.jsonl` files, unchanged. The names above are the file ids; `lore.toml`'s `areas` list carries each area's `description` and its `parent` (`""` for `general`, `general` for the folder areas, and no parent for the team areas — they are the project's own, not a folder's). `lore areas` prints the tree; `find`/`for` may filter by area or by a subtree.
- **At store creation:** `general` and one area per catalog folder (section 3.1); a team area is added when that team is created. Renaming a folder keeps its area (the slug is derived from the folder's stable path or catalog id, not its label); removing a folder from the project keeps its area and records.

### 4.5 When the team ends (answer 4)

- When a team finishes (Finish, Stop or Archive), its workspace is kept as it is and marked read-only (`teams/<id>/.closed`). Its records stay searchable from the project, and the team area of section 4.4 keeps them addressable.
- **Export into the project** happens only in two ways, never otherwise:
  - **The project's rule:** the project setting `export_finished` (section 3.1) is set by the owner (Memory > "Export finished teams", off by default). When it is on, finishing a team runs the export once, with duplicates skipped by the rule below; the commit names the rule (`lore: export team <name> under the project's export rule`).
  - **The owner's request:** Projects > Memory > Export, or the finished team's Memory tab, on the owner's click.
- **What it moves:** faults, decisions, readings, topics and recipes go from the workspace into the team's area `team.<slug>` of the project store; plans and items stay in the workspace as the team's history.
  - It is one write, under the lock, as one commit "lore: export team `<name>` (N records)".
  - Duplicates against the project store are checked first. Under the project rule the duplicate (section 1.1's signal) is skipped and the new records are written, and the owner is told how many were skipped; by the owner's request the owner picks keep-both, skip, or "supersede the older".
  - The export is idempotent by record id: running it again writes only what is new.
- **A finished team's workspace is never moved or deleted:** it is kept, read-only and searchable, whatever the project rule says.

### 4.6 A standalone team store (the `team` mode without a project store)

When the project has no store and a team's mode is `team`:
- The team gets its own store at `<state root>/lore/teams/<team-id>/` on the team's computer.
  - Its git is that store folder's own, and its privacy is as for a project store.
  - Its reference roots are the team's folder and its members' folders on that computer.
  - Its areas are the project's hierarchy as far as it exists for the team (a `general` and the team's own `team.<slug>`); a folder area appears only when the project's folders are known to that computer.
- At the end it is kept, read-only and searchable, like a workspace.
- When the project later turns memory on, the Memory panel offers to **export** each kept standalone store into the project store (section 4.5's export), by the owner's request or by the project rule if it is on.

### 4.7 The project's memory switch and the team modes

- **Turning the project's store off:** a team in `project` or `read-only` falls back to `team` when it has a workspace (the workspace stays local and searchable) or to `plain` when it has none; the lead is told once and the fallback is a normal mode switch (section 4.2), so the owner can confirm a different mode instead. Teams in `team` with a workspace inside the project store keep it read-only until the project is on again.
- **Turning it on:** teams in `plain`/`off` stay as they are until the owner switches their mode; teams in `team` with a standalone store keep it, and the Memory panel offers the export.
- **The mode decides, the model decides where:** a team's mode (4.1) decides what it reads and writes; the project's store model (3.4 or 3.5) decides where the store lives and how its writes reach other computers. A synced project does not change any team mode.

## 5. (d) Plain memory, exactly

Plain memory is the `plain` mode of section 4.1: what Zerus keeps for a team without a lore store. It is today's record, plus one generated digest:
1. **The team's own data in `teams.sqlite3`:**
   - goal, plan, phases and tasks, with criteria, status, decider and reason;
   - attempts with reports, commits and checks;
   - findings;
   - proposals and their decisions;
   - phase acceptance and send-back comments;
   - the journal events.

   It is kept as long as the team record is (archived teams included).
2. **Test results** in `teams/tests.sqlite3`, bounded as today (per suite, and per test since task 88).
3. **Iron rules** of the project, team, members and sessions, and estimates history.
4. **New: the team digest.**
   - **Content:** one generated text of at most 6 KiB.
     - The plan with each task's outcome: accepted, rejected or cancelled, and why.
     - The owner's decisions with their reasons.
     - Open and fixed findings.
     - The last test result per suite.
   - **Use:** it is built from items 1-3 on demand. It is shown in the lead's inbox after a compaction (section 6.2) and given in `hgs team status --digest`.

Plain memory has no search, no records of other teams and nothing across projects. It is not written to any file outside `teams.sqlite3`, and it ends with the team's data. A continuation team (`previous`/`next`) reads its predecessor's digest.

## 6. (e) What Teams writes and what agents read

### 6.1 Automatic writes

Teams writes only records that pass validation, and writes them as `agent:zerus` with the trailer `Zerus-Team`. A record is written automatically as soon as **every required field is known** (answer 5): a fault whose symptom, cause, fix and check are all present is written at once, without waiting for the lead. A record whose required fields Zerus cannot fill is never written half-filled. Instead it becomes a lead inbox item ("write the fault for #12: cause, fix, check"), with the drafted fields as a ready command.

| Teams event | Lore record (workspace) |
|---|---|
| A plan is approved | `plan` (title = team name and phase, goal = team goal, status live) and one `item` per task. Criteria come from the task, `deps` from the task's deps (hard or soft). |
| A phase or plan is extended, or a task is added | new `item`s in the same plan |
| A task is accepted | `item done`, with `commits` from its attempts' commits and `outcome` from the decision reason (first 1000 characters) |
| A task is cancelled | `item drop`, with `reason` and `decided_by` (Lead, You or Zerus) |
| A task is held | `item defer` with the hold reason; a release becomes `item reopen` with the reason |
| A reopen or rework | `item reopen` with the reason |
| The team finishes | `plan close`, with `outcome` from the finish summary |
| An owner decision: a proposal allowed or denied, a phase accepted or sent back, a replan or plan-mode switch | `decision`: question = what was proposed, choice = allowed or denied plus the change, reason = the owner's reason (or "no reason given"), `decided_by` = the owner |
| A review finding fixed by a task, or a rework whose cause the reviewer named | a `fault`, written automatically when all four fields are known: symptom = the finding's scenario, cause = the reviewer's reason, fix = the fixing task's report summary, check = the test that now passes. A missing field goes to the lead's inbox as above, and the record is written as soon as the field arrives. |
| A test suite's result, with a time measured on a commit | `reading`: what = suite duration or pass count, command = the suite's command, value, `measured_at` = commit. A new reading supersedes the previous current one of the same suite. |

Writes are batched per dispatcher tick: one write per team per tick, one commit. They go through the writer lock, and through `@home` when the store is elsewhere. A write that is refused (for example a duplicate) is recorded in the journal as `lore_refused` with the reason and never retried blindly.

### 6.2 What the lead and members read

All reads are bounded, and none blocks a hand-out: a slow or unreachable store adds a one-line note instead. What a team reads follows its mode (section 4.1): `team` reads its workspace; `project` reads the workspace and the project store; `read-only` reads them without writing; `plain` reads the digest; `off` reads neither.
- **Guide:** `hgs guide lore` is RustCodeLore's `guide.md` with Zerus's addressing (`--project`, `--team`). The lead's brief and every hand-out name it in one line.
- **Hand-out block "Project memory"** in `hand_off_text_rules`, after the rules block, when the mode has a store. It holds the top 5 of `lore for <the task's files>` and `lore find <the task's title>`, as titles and ids, at most 1.5 KiB — from the workspace in `team`, from both in `project`. The member runs `hgs lore show <id>` for more.
- **Inbox digest:** records written since the lead's last inbox message, at most 10 lines, as one inbox item.
- **Recall after compaction:** the SessionStart hook's `additionalContext` for a team session, sources `compact` and `clear`:
  - with a store (`team`, `project`, `read-only`), up to 2 KiB of the workspace's live plan state and the newest decisions and faults (plus the project's newest in `project`/`read-only`);
  - with `plain`, the team digest;
  - with `off`, nothing beyond the journal the session already has.

## 7. Desktop

Qt and Zerus Next get the same surfaces:
- **Projects > Memory:** on or off, the home computer, the folder (Zerus's default or chosen), Move…, record counts by kind, the refused-privacy state, the exports, the **area tree** (general, the folders, the teams), and two switches of the owner's:
  - **Synchronise between computers** (section 3.6, off by default: the home model);
  - **Export finished teams** (the project rule of answer 4, off by default).
- **New team > Memory:** the starting mode (section 4.1), with only the modes the project allows: `project` preselected when the project's store is on, `team` when it is not, `plain`/`off` always, `read-only` as an observer. The form says what the chosen mode writes and reads in one line.
- **Team > Memory tab:** the mode with its switch (section 4.2, owner-confirmed: the current and new mode, the writes affected and the workspace's fate), the workspace's plans, items, decisions and faults as read-only lists with search, the team's area in the project, and **Export** when the team has ended (sections 4.5, 4.6).

## 8. (f) Conformance and measurement, then the replacement (answer 6)

The Python version is the reference until every point below holds. Then RustCodeLore is CodeLore 1.0: it moves to github.com/WhiteWh/CodeLore (section 2.1), the Python 0.3.0 tree is retired there (a tag keeps it for the record), and Zerus's dependency switches from the in-tree path to the released implementation. The Rust tool, its guide and its index are the only ones from then on; a machine that had the Python tool gets the Rust one in the same upgrade, and the Python index is removed with the old tool.
1. **Format, byte for byte:** a corpus of stores is written by Python 0.3.0, read by Rust, rewritten with `lore fmt`, and compared bytes-equal. The reverse direction is checked too. The corpus is CodeLore's test fixtures plus the RESULTS.md corpus and one store per kind and status. Every refusal message for a malformed file matches.
2. **The Python test suite:** the 68 tests of `tests/` run unchanged against the Rust `lore` binary. A shim makes `RepoCase` call the binary instead of the module. A test that imports Python internals instead of the CLI is ported to an equivalent Rust test, listed in a mapping table. All 68 must pass.
3. **CLI parity:** for every command and flag of section 1.1, the same inputs on the same store give the same exit code and the same `--json` output. Floating scores are compared within 1e-6 and ordering must match. A recorded script of about 300 invocations runs against both.
4. **Retrieval, RESULTS.md's method:**
   - The same corpus (37 records among 1,490 rows) and the same 82 questions.
   - The same metrics: first, top 3, top 10, MRR, bytes read (median and p90), calls and wall time per call.
   - Runs with and without embeddings.
   - The bar: every quality metric at least the Python value (ties allowed), and wall time per call at most Python's 0.75 s (0.6 s text only).
   - The numbers go in `crates/lore/RESULTS.md` with the date, machine and commit.
5. **The agent test** RESULTS.md names as not yet run:
   - Two sessions solve the same 10 recorded faults, one with `hgs lore` and one with grep only.
   - The comparison counts tool calls, bytes read and correct fixes.
   - It is run once the port passes points 1-4, and reported the same way.
6. **Zerus's own tests:**
   - the writer lock under two processes;
   - the move (copy, verify, switch, failure at each step leaves the old store);
   - privacy refusal of a chosen folder;
   - `@home` reads and writes with a retried request id;
   - each section 6.1 mapping from a fake team's events, including a fault written automatically once its fourth field arrives;
   - the memory modes and the owner-confirmed switch (section 4.2): the same workspace across `team`/`project`/`read-only`, the fallbacks of section 4.7, and no write in `read-only`;
   - the areas hierarchy (section 4.4) at store creation and with a team area added and kept;
   - the export by the project rule and by the owner's request, and none otherwise;
   - the bounded hand-out block and recall text, per mode (section 6.2);
   - a synced store (section 3.5) with two replicas: concurrent appends merge, a same-id conflict raises the owner's item, the settings' last writer wins, and switching to and from sync keeps every record.
7. **The replacement:** the crate's move to the CodeLore repository, its CI and release, the Python tree's retirement tag and the in-tree path dependency's removal, checked by building Zerus against the released crate. This point is what makes answer 6 done; until it holds, both tools stay installed and the Python one is the reference.

## 9. Order of work

### Phase 4: memory in Zerus

1. The `crates/lore` port with points 1-3 of section 8, in this repository.
2. Retrieval parity (point 4).
3. Project settings, store creation with the area hierarchy (section 4.4), privacy and `hgs lore --project` locally, then over `@home`.
4. The move.
5. Team workspaces, the five memory modes and the owner-confirmed switch (sections 4.1, 4.2), and standalone team stores.
6. Teams' automatic writes (section 6.1, with a fault written as soon as its fields are known) and the reads per mode (section 6.2).
7. The desktop surfaces: the mode switch, the area tree, the project's export rule and the sync flag.
8. The agent test (point 5).

### Phase 5: synchronisation and the replacement

9. Synchronisation between computers (sections 3.5, 3.6) with the Loro replicas and the conflict rules, tested with two replicas.
10. CodeLore 1.0: the crate's move to github.com/WhiteWh/CodeLore, the Python tree's retirement (section 8, point 7) and Zerus's dependency switch.
11. The conformance points re-run against the released crate and the owner's real stores.

### The phase-4 and phase-5 notes the answers change

- **Phase 4, task 5:** the team's memory is five modes with an owner-confirmed switch (answer 1), not a plain/RustCodeLore creation choice. The New team form picks the starting mode; the Team > Memory tab switches it while the team runs, like task 58's plan mode.
- **Phase 4, task 3:** the store starts with the area hierarchy (answer 3): `general`, one area per project folder, and one `team.<slug>` under the project per team created — the team area is not under a folder.
- **Phase 4, task 6:** a fault is written automatically once every field is known (answer 5); only a record with a missing required field still reaches the lead's inbox.
- **Phase 4, task 7:** the project's `export_finished` rule (answer 4) joins the owner's Export; a finished team is never exported without one of them, and its workspace is kept and searchable either way.
- **Phase 5 is new in this plan:** it carries the sync model (answer 7) and the replacement of the Python tool (answer 6). The port's move to the CodeLore repository and the retirement are its last task, so phase 4 ships with the in-tree crate and the Python reference still installed.
- The retrieval and agent tests stay in phase 4 (points 4-5); the sync tests are phase 5's, because they need two computers' replicas.

## 10. Record of the answers

The seven questions of task 122 were answered by the owner on 2026-11-07. Section 0.1 is the record of the answers and where each is designed; this document is amended to them and has no open questions. A question that the implementation raises goes to the owner as its own card, not into this document.
