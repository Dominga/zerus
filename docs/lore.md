# Project and team memory: RustCodeLore in Zerus

Status: specification (task 122, owner request of 2026-11-07). Nothing here is
implemented yet. RustCodeLore is a Rust port of the owner's CodeLore
(`D:/Tools/CodeLore`, Python 0.3.0, github.com/WhiteWh/CodeLore). The Python
version stays the reference until the Rust one matches it (section 8).

## 0. The owner's decisions and where this document answers them

| The owner's point (2026-11-07) | Design |
|---|---|
| (1) The user decides whether a project uses project memory. When a project is declared in Zerus (a project may group several folders or repositories), RustCodeLore can be turned on for the whole project, and the user chooses where its database lives: by default under Zerus's own data folder, or another folder. | One store per catalog project. The project's memory setting lives in the project's replicated settings, so every computer of the swarm sees it. The default path is under the state root; another folder can be chosen, and the store can be moved with a confirmation and no data loss (section 3). |
| (2) When a team is created and its project has RustCodeLore on, the team gets a shared workspace in it by default. | A team workspace is a sub-store inside the project store. The team writes there; reads span the workspace and the project store (section 4). |
| (3) When the project has it off, New team offers the memory mode like the behaviour mode: plain memory or RustCodeLore, RustCodeLore by default. | New team gets a Memory choice beside its other mode choices, defaulting to RustCodeLore. With RustCodeLore and no project store, the team gets a standalone team store (section 4.3). Plain memory is defined exactly in section 5. |

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
- **New team** has no "behaviour mode". Its mode-like choices are Planning (lead or template), Folder policy and Later-phase approval. The Memory choice joins them; open question 1 asks which choice the owner meant.

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

- **Binaries:**
  - `lore`, a standalone CLI with the Python CLI's commands, flags, exit codes and `--json` shapes, byte-identical for the same store. Section 8 holds it to that.
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

CodeLore has none: concurrent writers rely on git's compare-and-swap. In Zerus one store serves several sessions and computers, so RustCodeLore adds one:
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

**A store under Zerus data (the default) is its own private git repository.** Zerus runs `git init` in the store folder when the store is created. This is only Zerus's own store folder under the state root; no project folder is ever initialized. Each write commits as CodeLore's `plain` mode does: the same subjects, bodies and trailers. So history, blame and recovery work as in CodeLore, without touching any project repository.
- `commit.mode` is `plain`, with no remote and no push. Moving the store keeps its history (section 3.3).
- The commit author is Zerus's `by`: the owner, or `agent:<model>` for a session. A trailer `Zerus-Session: <name>` names the session, and `Zerus-Team: <id>` the team when one wrote.

**A store in a chosen folder:**
- **Inside a git repository:** CodeLore's behaviour exactly, `plain` by default. Its `lore.toml` may say `shared-tree` or `none`. Zerus never pushes unless `lore.toml` says `push = true`.
- **Outside any repository:** Zerus initializes the store folder itself as its own repository, as under Zerus data. It does so only after the owner chose that folder and confirmed it, and never in a folder that is inside another working tree.

A store whose git is unusable (no git on that computer, a broken repository) is read-only, with a note naming why. Writes are refused, never written uncommitted.

### 2.6 The index

- **Location:** under the user cache as in CodeLore, keyed by the store's real path. On Windows `%LOCALAPPDATA%\codelore\<key>\index.sqlite`, so the Python and Rust tools can share the folder.
- **Rust's own index:** RustCodeLore writes `index-rs.sqlite` beside it. Each index is disposable and versioned by `user_version`; one tool never reads the other's. Sharing one index is open question 6.
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
lore: { enabled: bool, home: <machine id>, path: <absolute path on home, or "">, workspace_default: true }
```

- **Turning it on** (Projects > the project > Memory) is the owner's action on the desktop, in Qt's ProjectsDialog and Zerus Next's Projects screen.
  - **Home computer:** the owner chooses the computer that holds the store, this one by default.
  - **Store folder:** Zerus's default, or another folder on that computer.
  - Zerus then creates the store: `lore.toml` with the areas from open question 3, `git init` (section 2.5), and the privacy checks (section 3.2).
- **The default path** is `<state root>/lore/<sha256(project_id)[..16]>/` on the home computer. `<state root>` is `HGS_STATE_DIR`, else `~/.local/state/hgs/sessions`. A project's id never changes, so renaming a project keeps its store.
- **Turning it off** keeps the store and its files, and stops Teams writing and reading it. Turning it on again finds the same store.

### 3.2 Privacy

A store may hold code facts and decisions from private repositories, so it is a **private store**:
- `private_stores::known()` lists `lore/` and every store folder under it.
- The same checks apply: owner-only on Windows (no reparse point, one link), 0700 and 0600 on Unix.
- Repair happens only inside the state root, as today. A chosen folder outside the roots is verified, never repaired. If it is not private, the store is refused and listed in `private-stores.json` with the reason. The desktop shows it, and the owner fixes it or chooses another folder.
- The index folder (`<state>/lore-cache`) is private too.
- Nothing from a store leaves the home computer except through `hgs @home lore …` answers to the owner's own computers (section 3.4).

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
- **Moving to another computer** (changing `home`) is the same copy over `hgs @alias`, with the same checks.

### 3.4 Which computer holds it, and remote members

- The store lives on its `home` computer only. Stores are never replicated: one writer lock, one history.
- **On the home computer:** `hgs lore` opens the store directly.
- **On any other computer:** `hgs lore …` with `--project` runs `hgs @home lore …` over the existing SSH transport (`swarm::remote_hgs`). Every computer of the swarm knows `home` from the replicated settings.
- **Requests:** every write is one request with a request id, so a retried write applies once. That needs a small `requests` table in the store's private folder, as Zerus's other write paths have. Reads are plain.
- **An unreachable home:**
  - Reads say "the project memory lives on `<home>`, which is not reachable".
  - Writes from Teams wait in the team's journal as `lore_pending` and are sent when `home` answers.
  - A person's own `hgs lore add` is refused with the reason.
- **Members' sessions** reach the store the same way, through their own computer's hgs. They need nothing installed beyond hgs.

## 4. (c) Team workspaces

### 4.1 A workspace inside the project store

- **What it is:** a sub-store at `<store>/teams/<team-id>/`, with its own `lore.toml` (`dir = "."`, the project's areas) and the same JSONL layout. It is committed in the store's repository, so one history covers the project and its teams.
- **Created** with the team when the project has memory on, which is the default (point 2). The New team form shows "Project memory: on (shared workspace)" with a switch to turn it off for this team.
- **Writes:** Teams' automatic records (section 6) and its agents' `hgs lore add` go to the workspace.
- **Reads:** `find`, `for`, `show` and `exists` read the workspace and the project store together. A result says which one holds it.
- **Ids:** ids are unique across both. A new record whose id exists in the project store is refused with CodeLore's "already exists" message.

### 4.2 When the team ends

- When a team finishes (Finish, Stop or Archive), its workspace is kept as it is and marked read-only (`teams/<id>/.closed`). Its records stay searchable from the project.
- **Export into the project** (the owner, from the finished team or the Memory panel) moves the workspace's knowledge records into the project's area files.
  - Faults, decisions, readings, topics and recipes go across; plans and items stay in the workspace as the team's history.
  - It is one write, under the lock, as one commit "lore: export team `<name>` (N records)".
  - Duplicates against the project store are shown first. The owner picks keep-both, skip, or "supersede the older".
  - Nothing is exported automatically. That is open question 4.

### 4.3 A standalone team store

When the project has no memory and New team's Memory is RustCodeLore (the default, point 3):
- The team gets its own store at `<state root>/lore/teams/<team-id>/` on the team's computer.
  - Its git is that store folder's own, and its privacy is as for a project store.
  - Its reference roots are the team's folder and its members' folders on that computer.
- At the end it is kept, read-only.
- When the project later turns memory on, the Memory panel offers to **export** each kept team store into the new project store (section 4.2's export).

## 5. (d) Plain memory, exactly

Plain memory is what Zerus keeps for a team without any lore store. It is today's record, plus one generated digest:
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

Teams writes only records that pass validation, and writes them as `agent:zerus` with the trailer `Zerus-Team`. A record whose required fields Zerus cannot fill is never written half-filled. Instead it becomes a lead inbox item ("write the fault for #12: cause, fix, check"), with the drafted fields as a ready command.

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
| A review finding fixed by a task, or a rework whose cause the reviewer named | a drafted `fault`: symptom = the finding's scenario, cause = the reviewer's reason, fix = the fixing task's report summary, check = the test that now passes. Missing fields go to the lead's inbox as above. |
| A test suite's result, with a time measured on a commit | `reading`: what = suite duration or pass count, command = the suite's command, value, `measured_at` = commit. A new reading supersedes the previous current one of the same suite. |

Writes are batched per dispatcher tick: one write per team per tick, one commit. They go through the writer lock, and through `@home` when the store is elsewhere. A write that is refused (for example a duplicate) is recorded in the journal as `lore_refused` with the reason and never retried blindly.

### 6.2 What the lead and members read

All reads are bounded, and none blocks a hand-out: a slow or unreachable store adds a one-line note instead.
- **Guide:** `hgs guide lore` is RustCodeLore's `guide.md` with Zerus's addressing (`--project`, `--team`). The lead's brief and every hand-out name it in one line.
- **Hand-out block "Project memory"** in `hand_off_text_rules`, after the rules block. It holds the top 5 of `lore for <the task's files>` and `lore find <the task's title>`, as titles and ids, at most 1.5 KiB. The member runs `hgs lore show <id>` for more.
- **Inbox digest:** records written since the lead's last inbox message, at most 10 lines, as one inbox item.
- **Recall after compaction:** the SessionStart hook's `additionalContext` for a team session, sources `compact` and `clear`:
  - with lore, up to 2 KiB of the workspace's live plan state and the newest decisions and faults;
  - with plain memory, the team digest.

## 7. Desktop

Qt and Zerus Next get the same surfaces:
- **Projects > Memory:** on or off, the home computer, the folder (Zerus's default or chosen), Move…, record counts by kind, the refused-privacy state, and the exports.
- **New team > Memory:**
  - when the project has memory: "Project memory: on (shared workspace)" with a switch;
  - otherwise: RustCodeLore (the default) or Plain memory.
- **Team > Memory tab:** read-only lists of the workspace's plans, items, decisions and faults, with search.

## 8. (f) Conformance and measurement

The Python version is the reference until every point below holds.
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
   - each section 6.1 mapping from a fake team's events;
   - the bounded hand-out block and recall text.

## 9. Order of work

1. The `crates/lore` port with points 1-3 of section 8.
2. Retrieval parity (point 4).
3. Project settings, store creation, privacy and `hgs lore --project` locally, then over `@home`.
4. The move.
5. Team workspaces and standalone team stores.
6. Teams' automatic writes (6.1), then the reads (6.2).
7. The desktop surfaces.
8. The agent test (point 5).

## 10. Open questions for the owner (one card)

```json
{"kind":"question","reason":"Spec task 122: project and team memory with RustCodeLore needs these choices",
 "question":"docs/lore.md (task 122): 1) New team has no 'behaviour mode' today; the Memory choice is designed like its Planning choice (lead or template). Is that the place you meant? 2) A store under Zerus data becomes its own private git repository so every write is a commit with history, as CodeLore does; acceptable? 3) The areas a new project store starts with: CodeLore's default 'general', or one per project folder? 4) When a team ends its workspace is kept and searchable; export into the project store only on your click (default), or automatically with duplicates skipped? 5) Teams writes plans, items, your decisions and test readings itself, and drafts faults for the lead to complete; should faults also be written automatically when every field is known? 6) The Rust and Python indexes stay separate files; or should RustCodeLore replace the Python tool on your machines once it passes the conformance bar? 7) The store lives on one home computer and others reach it over hgs; is a home computer that can be offline acceptable, with team writes waiting until it answers? Answer the numbers you want changed, or accept the defaults.",
 "options":["Accept all defaults","I answer some numbers in my own words","Hold the work until we talk"],
 "recommended":"Accept all defaults"}
```
