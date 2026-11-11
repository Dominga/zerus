# Project and team memory: RustCodeLore in Zerus

Status: specification (task 122, owner request of 2026-11-07; amended by the
owner's answers to its seven questions, task 141, 2026-11-07). Nothing here is
implemented yet. RustCodeLore is the Rust implementation of CodeLore
(`D:/Tools/CodeLore`, Python 0.3.0, github.com/WhiteWh/CodeLore): it starts as a
port in `crates/lore` here and is now the implementation itself, in CodeLore's own repository (section 2.1).
The Python version stays the conformance reference until the Rust one matches
it, then it is retired (section 8). Section 11 (task 131, owner, 2026-11-07)
adds the working tier: a fast, expiring memory beside the durable lore, for
pulses, messages and briefs.

## 0. The owner's decisions and where this document answers them

### 0.1 The seven answers (2026-11-07)

| # | The owner's answer | Design |
|---|---|---|
| 1 | Memory gets several behaviour modes, like a team's plan modes standard and free — not a Planning-like choice. | A team's memory mode: `plain`, `team`, `project`, `read-only` or `off`, chosen at creation and switched while it runs like task 58's plan mode, owner-confirmed (sections 4.1 and 4.2). |
| 2 | A store under Zerus data is its own private git repository: yes. | Section 2.5: `git init` only in Zerus's own store folder, `plain` commits, no remote. |
| 3 | The areas are a hierarchy: General (descriptions and shared quick facts across the project's folders), then one area per project folder, then one per team created under the project (under the project, not under a folder). | Section 4.4: the project store's area tree with `general` at its root, `folder.<slug>` areas and `team.<slug>` areas under the project; section 3.1 creates them. |
| 4 | A finished team's workspace is kept and searchable; it is exported into the project store only by a project rule or the owner's request. | Section 4.5: kept read-only and searchable; the project's `export_finished` rule or the owner's Export; never automatically otherwise. |
| 5 | Faults are written automatically when every field is known. | Section 6.1: a fault is written as soon as symptom, cause, fix and check are known; only a record with a missing required field becomes a lead inbox item. |
| 6 | RustCodeLore fully replaces the Python CodeLore: CodeLore is rewritten in Rust; the code lives in github.com/WhiteWh/CodeLore (it started as `crates/lore` here; Zerus depends on it, task 256); Python is the conformance reference until parity, then retired. | Sections 2.1 and 8: the crate is the working implementation now, becomes CodeLore 1.0 in its own repository, and the Python 0.3.0 tree is retired after the conformance points hold. |
| 7 | The home-computer model is a flag: by default writes wait for the home computer; the owner can ask for synchronisation between computers instead. | Sections 3.4-3.6: `sync = false` (the default, home computer) and `sync = true` (Loro replicas in `crates/replica`), with the conflict rules of section 3.5 and the owner-confirmed switch of section 3.6. |

### 0.2 The earlier points (2026-11-07, task 122)

| The owner's point (2026-11-07) | Design |
|---|---|
| (1) The user decides whether a project uses project memory. When a project is declared in Zerus (a project may group several folders or repositories), RustCodeLore can be turned on for the whole project, and the user chooses where its database lives: by default under Zerus's own data folder, or another folder. | One store per catalog project. The project's memory setting lives in the project's replicated settings, so every computer of the swarm sees it. The default path is under the state root; another folder can be chosen, and the store can be moved with a confirmation and no data loss (section 3). |
| (2) When a team is created and its project has RustCodeLore on, the team gets a shared workspace in it by default. | A team workspace is a sub-store inside the project store. The team writes there; in the `project` mode reads span the workspace and the project store (section 4.3). |
| (3) When the project has it off, New team offers the memory mode like the behaviour mode: plain memory or RustCodeLore, RustCodeLore by default. | New team picks the team's initial memory mode (section 4.1): `project` when the project's store is on, else `team`; `plain`, `off` and `read-only` where allowed. Memory is a mode of the running team, not a Planning-like creation choice (answer 1). |

### 0.3 The working tier (2026-11-07, task 131)

| The owner's point or review feedback | Design |
|---|---|
| A fast working memory beside the durable lore, expiring by itself, for member pulses, team messages and handoff briefs; the durable tier stays CodeLore's strict records. | Section 11: three kinds in the store's SQLite only (never JSONL or git), each with an expiry (11.3), bounded (11.4), found by the same `find` ranked lower and marked working (11.6), promoted to a durable record under the strict rules (11.8); plain-memory teams keep the same kinds in Zerus state (11.5). |
| (a) Verbatim text is cheap to store and costly to read; pulses are the default view and `find` also searches the verbatim turn history, federated into Zerus's content-search index, with retention, size bounds, redaction and a per-session opt-out. | 11.2 (the current pulse per member) and 11.7 (the history provider, never copied, bounded, redacted, scoped per caller, opt-out per session). |
| (b) Duplicates and conflicts between members. | 11.10: the check runs inside the writer lock and returns candidates; the writer adds an occurrence or supersedes on request; sync mode keeps section 3.5's rules. |
| (c) Message urgency: normal and urgent, urgent marked in the doorbell. | 11.11. |
| (d) Brief and pulse quality as acceptance tests. | 11.15, tests 1 and 2. |
| (e) The interface is the CLI (`lore`, `hgs lore`, `--json`), no MCP server. | 11.13. |
| (f) Briefs survive a home-computer restart. | 11.12: the latest brief per task is a committed file (or a `teams.sqlite3` row); the TTL governs the older versions. |
| (g) Secrets: verbatim history never in the store's git or the Loro replica; every durable write and promote passes a secret scan; scrubbing terminal output is best-effort. | 11.1 (hard rules 1 and 2) and 11.9. |
| (h) The lead's acceptance test after a forced compaction. | 11.15, test 3, with `hgs team lore plans` (11.13). |
| (i) The lore/Zerus boundary: the crate never depends on Zerus; history search is a provider trait Zerus implements. | 11.14. |
| (j) Urgent messages during a long turn: delivered at the next safe point, never pasted into a working composer; the lead is told past a bound and may stop the member natively. | 11.11 and hard rule 3. |

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
  - It holds FTS5 `words` (porter) and `terms` (code tokens), plus optional vectors (one per record text, by the text's hash and the model's name).
  - It has CodeLore's schema (`user_version` 3), so the Rust and the Python versions read each other's file.
  - A file is read again only when its size or mtime moved and its hash differs; a file that is gone is dropped, and another version rebuilds it.
  - When the cache cannot be written, the index goes to the repository's git directory (`.git/codelore`) and the command says so.
- **Search:** hybrid, fused by RRF (k = 60, depth 50).
  - The lists are BM25 words, BM25 terms and vectors. The vectors come from the built-in model, run in the process (no Ollama). A store Zerus makes names `model = "auto"`: bge-m3 with a GPU, multilingual-e5-base on the CPU, both multilingual (Russian included); the English-only `nomic-embed-text` remains selectable by name. They are optional: without the model, or with `embed.provider = "none"` or `--text-only`, the search runs on the first two lists and says so.
  - The weights (about 550 MB) are not in the binary and never fetched by a search: `lore model fetch` downloads them from Hugging Face at a pinned revision into the user cache (or an offline install gives `LORE_MODEL_DIR`), each file checked by size and SHA-256.
  - Records are embedded with the model's `search_document: ` prefix and queries with `search_query: `.
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

- **Crate:** the `codelore` library (github.com/WhiteWh/CodeLore, `crates/lore` here until task 256) with the same modules as the Python package. hgs does not link it (see "hgs runs the binary" below):

  | Module | Role |
  |---|---|
  | `config` | `lore.toml`, unknown keys refused |
  | `schema` | kinds, fields, limits, statuses, `validate` |
  | `store` | canonical JSONL, order, placement |
  | `gitio` | git plumbing and the three commit modes |
  | `ops` | the one write path, references, relations, ids, commit subjects |
  | `dedupe` | duplicates |
  | `index` | SQLite FTS5 and vectors |
  | `embed` | the built-in nomic-embed-text model (candle), `builtin` or `none` |
  | `search` | `find`, `for` |
  | `plans` | counts, layers, readiness, stale |
  | `cover` | sources |
  | `guide` | the agent guide text, embedded |

- **The crate is the implementation (answer 6; the move is done, task 256).**
  The Rust lore lives in github.com/WhiteWh/CodeLore (package `codelore`), with
  its own CI, releases and guide, and Zerus has no copy of it: `crates/lore` is
  gone. Zerus builds the `lore` binary it ships from a pinned git revision of
  that repository, in the small package `crates/lore-bin` (bin `lore`, one line
  of `main`, `codelore::app::main()`), whose own `Cargo.toml` names the
  dependency (`zerus-lore = { package = "codelore", git = ..., rev = ... }`, so
  `use zerus_lore::...` stays) and whose `Cargo.lock` pins it, so a build is
  offline after the first fetch and CI caches key on that lock. hgs's own
  `Cargo.toml` never names it: Cargo unifies features across one build, and
  linking the crate would switch serde_json's `preserve_order` on for hgs (see
  "hgs runs the binary"). Packaging (`scripts/package-*.py`, `install.sh`,
  `packaging/desktop/install.sh`, `mac-install.sh`, the AUR recipes, the CI workflows) builds
  `crates/lore-bin/Cargo.toml` and reads the shipped version from its lock
  (`scripts/lore_dependency.py`).
- **Changing lore.** Commit the change in the CodeLore repository, with its tests
  there (`cargo test --locked`, `cargo clippy --all-targets -- -D warnings`: the
  lore suite runs in CodeLore, not here). When it is pushed, move Zerus to it:
  set `rev` in `crates/lore-bin/Cargo.toml` to the commit, run
  `cargo update -p codelore --manifest-path crates/lore-bin/Cargo.toml` (it
  changes only the `codelore` entry of that lock, plus any dependency the new
  commit needs), and run `cargo test --locked --manifest-path crates/lore-bin/Cargo.toml`.
  Nothing else in Zerus changes for a lore change. The secret-scan vectors are
  the one test file both repositories hold: Zerus's `tests/fixtures/secret-scan-vectors.json`
  and CodeLore's `tests/fixtures/secret-scan-vectors.json` must stay equal, and
  each repository pins the same SHA-256 of the file (its LF form) in a test
  (`crates/lore-bin/tests/lore.rs` here, `tests/secret_vectors.rs` there); a change
  to the vectors is made in both with the constant. The file stores every sample in a
  real token format split by a `<split>` marker after its prefix, and each reader joins
  it, so that no literal in a real token format is in either repository (GitHub secret
  scanning flags them); a test that needs such a text writes it split in the source
  (`concat!("ghp_", "...")`) and never as one literal.
- **hgs runs the binary (task 128, lead's decision of 2026-11-07).** hgs
  calls the `lore` binary as a subprocess instead of linking the crate, for
  two reasons:
  - The crate needs serde_json's `preserve_order` feature (records keep their
    field order byte for byte). Cargo unifies features across one build, so
    linking it would turn `preserve_order` on for all of hgs and change the
    key order of every JSON value hgs writes and hashes: request receipts,
    card hashes and stored team records.
  - The built-in embedding model (candle) is heavy, and hgs would carry it
    whether or not a project uses memory.

  hgs finds the binary as `HGS_LORE`, else `lore` beside the hgs executable,
  else on `PATH`. Without it a team keeps plain memory and its journal says
  so once (`lore_unavailable`). hgs passes a store's reference roots
  (section 2.4) in `LORE_ROOTS`, the platform's path list. Release packages ship
  `lore` beside hgs in the Windows zip, the macOS app and the Linux installers;
  model weights are fetched at runtime and are never packaged.
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
- Any embedding provider other than the built-in model (Ollama was dropped by the owner's decision), and any network model service. A lore.toml that still says `provider = "ollama"` reads as `builtin`, and its `url`, `model`, `timeout` and prefix keys are ignored.
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

### 3.3a A store in the project's repository (2026-11-11)

A project may keep its records in its own repository instead (the owner of RogueDOD chose this): the git root holds `lore.toml` with real areas, `lore/` is tracked, and `[commit] mode = "shared-tree"` commits into the working branch. CodeLore's root is always the git root (refs and the Atlas count from it; a `lore.toml` in a subfolder is not supported), so Zerus looks up from the project's folder, a team folder or a member's folder to the top of its working tree. That `lore.toml` is the repository store when it names an area other than the placeholder `use-the-project-store` and the tree is not a private store Zerus made. Then:
- `hgs team lore find|show|plans` read it, beside a team's own workspace, and outside any running team from the caller's folder; each hit is marked `repository`.
- `hgs team lore export --team ID [--area AREA]` writes the team's faults, decisions and readings there through `lore add` (lore's schema checks, duplicates, refs on HEAD, its commit mode and trailers). Zerus never edits the repository's `lore.toml`: the area is `--area`, or the team's area when the repository names it.
- `hgs guide code` and `hgs guide team-lore` end with what the caller's `lore.toml` says, and a hand-out names `lore -C <git root>`.
- The session start's digest and the pre-edit hook (section 6.2) come from the repository, each not at all when its own `.claude/settings.json`, `.claude/settings.local.json` or `.codex/hooks.json` already runs it.
- `LORE_ROOTS` is not needed: lore checks refs in the repository that holds the store.

- A task's `output` may name a lore area, `lore:AREA` (an area id: letters, digits, `-`, `_`). It is an exact resource like a file (task 155): two open tasks with the same area are never handed out at once, plan validation asks to chain them, and the member's hand-out names the area as its own. Lore's write lock keeps parallel writes from losing records; the area output keeps two members from filling one area with near-duplicates.

A team's own workspace stays under Zerus data in this mode; the export is how its knowledge reaches the repository.

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

Writes never run on the dispatcher tick (task 128). A background thread of the team worker looks at the active teams every 30 seconds. For a team whose records changed since its last complete pass it reads the workspace's JSONL (at most 4 MiB), computes the difference and writes at most 50 changes: one `lore add any --new` for the new records, one `lore update --batch` for the changed ones, and `lore plan close` or `lore item reopen` where a status move needs that command. Each is one commit. Record ids are fixed by the team (`t<team8>-phase-<n>`, `<plan>#<task>`, `t<team8>-<proposal>`, `t<team8>-finding-<id>`, `t<team8>-rework-<task>-<attempt>`, `t<team8>-reading-<template>`), so a pass repeated after a crash writes nothing twice. Faults, decisions and readings go in the team's area `team-<team8>`, added to the workspace's `lore.toml` when it is missing. A decided item stays as it was decided, and a closed plan stays closed. A write that is refused (for example a duplicate) is recorded in the journal as `lore_refused` with the reason and never retried until the record changes. When lore did not run at all, nothing is remembered and the next pass tries again.

### 6.2 What the lead and members read

All reads are bounded, and none blocks a hand-out: a slow or unreachable store adds a one-line note instead. What a team reads follows its mode (section 4.1): `team` reads its workspace; `project` reads the workspace and the project store; `read-only` reads them without writing; `plain` reads the digest; `off` reads neither.
- **Guide:** `hgs guide lore` is RustCodeLore's `guide.md` with Zerus's addressing (`--project`, `--team`). The lead's brief and every hand-out name it in one line.
- **Hand-out block "Project memory"** in `hand_off_text_rules`, after the rules block, when the mode has a store. It holds the top 5 of `lore for <the task's files>` and `lore find <the task's title>`, as titles and ids, at most 1.5 KiB — from the workspace in `team`, from both in `project`. The member runs `hgs lore show <id>` for more.
- **Inbox digest:** records written since the lead's last inbox message, at most 10 lines, as one inbox item.
- **Project memory at every start** (2026-11-11): the SessionStart hook's `additionalContext` of every own session start (`startup`, `clear`, `compact`, team or not) ends with `lore -C <store> digest` (faults and decisions in force, titles only, at most 2000 bytes), in the room the iron rules, the hint and a team's recall leave. The store is the repository's (section 3.3a), else the project's Zerus store when its memory is on. A missing or slow lore (2 seconds) adds nothing.
- **Before an edit** (CodeLore 92a1c04): a Claude session's PreToolUse on Edit, Write, MultiEdit or NotebookEdit passes the event to `lore -C <store> hook pre-edit`, which names the records in force about that file, once per file and session, as the hook's `additionalContext` (at most 1.5 KB). The store is chosen once at the session's start (the same rule as the digest) and kept in its record; lore runs after the state lock is released, 2 seconds at most. Zerus calls both hooks itself, so it sets no `LORE_STORE` and a committed settings file names no machine's path; a repository whose own hooks already run `lore hook pre-edit` or `lore hook session-start`/`lore digest` gets neither from Zerus.
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
   - The numbers go in CodeLore's `docs/RESULTS-rust.md` with the date, machine and commit.
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
8. **The working tier** (section 11), independent of the replacement: the twelve acceptance tests of section 11.15, and the format check for `seen` (a file with the field is rewritten by Python 0.3.0 with the field kept, and the Rust and Python `find` of a store with no working entries are still byte-for-byte equal).

## 9. Order of work

### Phase 4: memory in Zerus

1. The `crates/lore` port with points 1-3 of section 8, in this repository (since task 256 in github.com/WhiteWh/CodeLore).
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

### The working tier: order of work (section 11)

It builds on phase 4's memory (the store, the writer lock, the modes, Teams' reads) and does not need phase 5's synchronisation; it ships after phase 4's item 7, in this order:
- **W1.** The crate: the working tables, the three kinds, expiry and bounds with a fake clock, the cleanup, the secret scan (11.9), `lore pulse|message|brief|working` and `--json` (tests 4, 5, 8 in part).
- **W2.** Search: the working lists in `find`, the rank rule, marks and flags, and `show <w-id>` (test 6).
- **W3.** Promote and the duplicate rules: `lore promote`, occurrences, `supersede`, and the `seen` field in `FORMAT.md` (tests 7, 9).
- **W4.** Zerus's pulses and briefs: Teams writes pulses at the points of 11.2, the lead's brief is written through `lore brief set` and committed under 11.12, `hgs team lore plans|pulses|brief`, plain-memory tables in `teams.sqlite3`, the hand-out line (tests 1, 2, 3, 12).
- **W5.** Messages and the doorbell: `Doorbell` and `TaskClock` implemented by Zerus, normal and urgent delivery at safe points, the bound, the lead's inbox item and the native stop request, `URGENT` in the three places (test 10).
- **W6.** The history source: `HistorySource` over the content-search index, scope per caller, the per-session opt-out, redaction (tests 8 and 11).
- **W7.** The desktop: Projects > Memory > Working memory (the three days settings and the urgent bound), the lead's pulse view, the session option `memory_history`, `URGENT` in the session row, in Qt and Zerus Next.
- **W8.** The acceptance runs 1 to 3 on recorded fixtures, then the owner's confirmation of section 11.16.

### The phase-4 and phase-5 notes the answers change

- **Phase 4, task 5:** the team's memory is five modes with an owner-confirmed switch (answer 1), not a plain/RustCodeLore creation choice. The New team form picks the starting mode; the Team > Memory tab switches it while the team runs, like task 58's plan mode.
- **Phase 4, task 3:** the store starts with the area hierarchy (answer 3): `general`, one area per project folder, and one `team.<slug>` under the project per team created — the team area is not under a folder.
- **Phase 4, task 6:** a fault is written automatically once every field is known (answer 5); only a record with a missing required field still reaches the lead's inbox.
- **Phase 4, task 7:** the project's `export_finished` rule (answer 4) joins the owner's Export; a finished team is never exported without one of them, and its workspace is kept and searchable either way.
- **Phase 5 is new in this plan:** it carries the sync model (answer 7) and the replacement of the Python tool (answer 6). The port's move to the CodeLore repository and the retirement are its last task, so phase 4 ships with the in-tree crate and the Python reference still installed.
- The retrieval and agent tests stay in phase 4 (points 4-5); the sync tests are phase 5's, because they need two computers' replicas.

## 10. Record of the answers

The seven questions of task 122 were answered by the owner on 2026-11-07. Section 0.1 is the record of the answers and where each is designed; this document is amended to them and has no open questions, apart from section 11.16, which lists the numbers and choices the working-tier addendum (task 131) took and the owner is asked to confirm. A question that the implementation raises goes to the owner as its own card, not into this document.

## 11. The working tier (addendum, task 131; owner, 2026-11-07)

Beside the durable lore, a fast working memory that expires by itself. It holds what agents are doing and saying now, for three uses: **pulses** (what a member does, so the lead sees it without reading chats), **messages** (a bus between members, the lead and the owner) and **briefs** (what a fresh session needs to take a task up). It is the efficient form of the owner's mempalace practice (duplicating chats so the lead sees what agents do, and a message bus), without copying transcripts again.

It builds on the memory of sections 2 to 6 and changes none of their rules: the durable tier stays CodeLore's strict records, byte for byte. Everything here is additive, and a store without working entries behaves and prints exactly as sections 1 to 10 say.

### 11.1 The tiers and the hard rules

| Tier | Holds | Lives in | Expires | Strict |
|---|---|---|---|---|
| **Durable** | faults, readings, decisions, topics, recipes, plans, items, sources | the store's JSONL, git, and (synced) the Loro replica | never by itself | yes: `validate()`, the duplicate check, the secret scan, one commit |
| **Working** | pulses, messages, briefs | one SQLite file of the store (section 11.5) | by rule (section 11.3) | bounded and typed, not a record |
| **History** | the verbatim turns of sessions | Zerus's own session histories and its content-search index, **not copied** | by Zerus's retention for those histories | no: it is searched, never stored by lore |

The hard rules, which every later section keeps:
1. **Verbatim text never enters the store's git or the Loro replica.** Not a transcript, not a terminal screen, not a pasted log. The working tier is never in JSONL and never in git, and it is not replicated (section 11.5). (A brief is authored text, not verbatim history, and is the one working-tier text that is committed, after the scan: section 11.12.) The only verbatim text that reaches a durable record is a quoted excerpt of at most 400 characters that a person or the lead's promote puts into a field, after the secret scan (sections 11.8 and 11.9).
2. **Every durable write and every promote passes the secret scan** (section 11.9) before the commit, and is refused with the match named. This includes Teams' automatic writes (section 6.1) and the durable brief (section 11.12).
3. **A doorbell never types into a working agent** (section 11.11). Nothing in this addendum pastes text into a composer of a session that is working.
4. **The interface is the CLI** (section 11.13): `lore` and `hgs lore`, `--json`. There is no MCP server. CodeLore never depends on Zerus (section 11.14).

### 11.2 The three kinds

All entries have an `id` (`w-<kind>-<8 hex>`, unique in the store), `kind`, `team` (the team id, or empty outside a team), `by` (the writing member, the lead, `owner` or `zerus`), `at` (UTC, seconds), `expires` (UTC, seconds, set by section 11.3 and moved by it), and `promoted` (the durable id it was promoted to, or empty).

**Pulse** — what a member did and does, in the four fields the lead reads:
- `task`: the team task reference (`#12`) or empty (a pulse outside a task); `done`, `next`, `blocked`: text of at most 500 characters each, at least one non-empty. `blocked` is empty when nothing blocks.
- It is written by Zerus at the points it already knows (a task starts, an interim report, a report, a stuck or waiting verdict, an acceptance, a rework) and by the member with `lore pulse add`. A pulse by the same `by` and `task` within 5 minutes of the previous one replaces it in place (its `at` moves); otherwise it is a new row. So a week of work is a bounded series, not a stream.
- The lead's default view is the **current pulse per member and task**: the newest row of each `(by, task)`. Older rows stay until they expire, so a reader can reconstruct a week (section 11.15).

**Message** — one note to a recipient:
- `to`: a member, `lead`, `team` (every member and the lead) or `owner`; `from` is `by`; `text`: at most 2,000 characters; `urgency`: `normal` (the default) or `urgent` (section 11.11); `state`: `queued`, `rung`, `read`; `rung_at`, `read_at`, and `safe_point` (how it was delivered: `idle`, `hook`, `turn_end`).
- A message to `team` is one row with one read mark per recipient (`read_by`), not one row per recipient.

**Brief** — what a fresh session needs to take up a task:
- `task`: the team task reference (required), `version` (1, 2, … per task), `text`: at most 8 KiB. The text is the lead's brief for the task: the goal, the files and constraints, what is decided, what is out of scope, the acceptance, and the first step.
- The **latest brief of a task is also a durable artefact** (section 11.12); the working row is the copy that expires.

### 11.3 Expiry, exactly

Times are UTC. An entry's `expires` is recomputed by the rule below whenever the thing it waits on changes, and the cleanup (section 11.4) deletes an entry whose `expires` has passed. No other rule deletes a working entry except the bounds of section 11.4.

| Kind | Expires at | Defaults (`lore.toml` `[working]`) |
|---|---|---|
| Pulse with a task | the task's close (accepted, cancelled, or the team's finish, whichever first) + `pulse_days`; while the task is open it has no expiry | `pulse_days = 14` |
| Pulse without a task | `at` + `pulse_days` | |
| Message | `read_at` + `message_days` once read by every recipient; `at` + `unread_days` while unread by anyone | `message_days = 7`, `unread_days = 30` |
| Brief, any version but the latest | the task's close + 0 days (the next cleanup) | |
| Brief, the latest | never by the TTL: its durable artefact governs (section 11.12) | |

- **A reopened task** clears the close time: its pulses and briefs go back to "while the task is open" and nothing that had not yet been deleted is lost. What was deleted is not restored.
- **An unread message that reaches `unread_days`** is not dropped silently: the cleanup writes one journal event `working_unread_expired` (sender, recipient, `at`, the first 80 characters) and, for a message to the lead or the owner, one inbox item with the same text, then deletes it.
- **The settings** `pulse_days`, `message_days` and `unread_days` are whole numbers from 1 to 365; a value outside that range refuses `lore.toml`. They may be lowered or raised per project (Projects > Memory > Working memory), owner-confirmed like the other memory settings.

### 11.4 Bounds and the cleanup, exactly

| What | Bound | At the bound |
|---|---|---|
| A pulse field (`done`, `next`, `blocked`) | 500 characters | the write is refused, naming the field and the length |
| A message `text` | 2,000 characters | refused, naming the length |
| A brief `text` | 8 KiB | refused; the lead is told it must be shorter (a longer subject is a document the brief names, or several topics) |
| Pulses per team | 2,000 | the cleanup deletes the oldest pulses of closed tasks first, then the oldest overall that are not a `(by, task)` current row |
| Messages per team | 1,000 | the oldest read messages go first; unread messages are never deleted for a bound |
| Unread messages per recipient | 50 normal, 5 urgent | the sender's write is refused naming the recipient and the count; the lead gets an inbox item (a recipient that does not read is a stall, section 11.11) |
| Brief versions per task | 20 | the oldest version goes; the latest never |
| Entries per store | 20,000 | as the lines above, store-wide |
| The file | 64 MiB | after the cleanup, the write that would pass it is refused with "the working tier is full (64 MiB)" and a journal event; reads and expiry go on |

- The settings may lower every bound and never raise it above this table (a larger value refuses `lore.toml`, as `[limits]` does for the format's ceilings).
- **Never deleted by a bound:** a current pulse, an unread urgent message, the latest brief of a task, an entry a promote is reading.
- **The cleanup is background and bounded.** It runs at store open and every 10 minutes while the store is in use, takes the store's working lock (not the writer lock; durable writes never wait for it) and deletes at most 500 rows per pass in transactions of at most 50 ms. It then runs `PRAGMA incremental_vacuum(200)`. A pass that finds more to do ends and the next one continues. It writes one journal event `working_cleaned` (count, bytes freed) per hour at most, and none when it deleted nothing. A cleanup that fails (a busy file, a full disk) is a journal event and a one-line note on the next read, never an error to a writer.
- **A read never waits for the cleanup**, and an entry past its `expires` is not returned even before the cleanup deletes it.

### 11.5 Where it lives, and what it is not

- **Storage:** `working.sqlite` in the store's private folder (section 3.4's `requests` table is in the same folder), WAL, `auto_vacuum = incremental`, `user_version` set. The folder is listed in the store's own `.gitignore`, no commit stages it, and a test asserts `git ls-files` never names it. It is a private store file (section 3.2: owner-only, no reparse point, one link).
- **Not disposable, unlike the index:** deleting it loses the working entries (and the latest briefs are safe, section 11.12). It has its own FTS5 tables (`words`, `terms`, built as the index's, section 2.6) over `text`, `done`, `next`, `blocked` and `to`/`by`.
- **Not replicated.** In a synced project (section 3.5) the working tier stays on the computer that holds it (the home computer in the home model; each computer for its own members' entries). Loro never carries it, so expiry never races a merge. A computer that is offline queues the messages it sends in Zerus's own outbox, as `hgs send` does, and the rows are written when it reaches the store; the content never travels except over the `@home` request path (section 3.4) that durable writes already use, with its request ids.
- **Plain-memory teams** (section 5) keep the same three kinds in Zerus state: tables `working_pulses`, `working_messages` and `working_briefs` in `teams.sqlite3`, with the same fields, expiry, bounds and cleanup (the team data's own file, so no store is needed), and one FTS5 table. They are searchable inside the team only (`hgs team lore find`); this is the single exception to section 5's "no search". A plain team's latest brief stays in `teams.sqlite3` as long as the team record does (section 5, item 1). `promote` is not available in `plain`: it answers "no store: this team's memory mode is plain; switching it is the owner's (section 4.2)". An `off` team has no working tier at all; its messages stay Zerus's present channels with nothing stored.

### 11.6 Search: found by the same `find`, ranked lower, marked working

`lore find` searches the working tier too, in the same call, and keeps CodeLore's contract for durable results.
- **Lists:** the durable lists are exactly section 1.1's (words, terms, meaning; phrase and signal bonuses). The working tier adds its own words and terms lists, over the working FTS5 tables, with the same BM25 weights and `RRF_K = 60`, depth 50.
- **Rank:** a working entry's fused score is **multiplied by 0.5** before it joins the durable results, and the phrase and signal bonuses never apply to it, so no working entry outranks a durable record that matches by signal or by phrase, and a durable and a working entry of equal rank order durable first. Ties are broken as the durable lists do.
- **Marks:** a working result carries `"tier": "working"` and `via` includes `working`. In text output it is prefixed `[working pulse]`, `[working message]` or `[working brief]` and shows `expires` as a relative time. A durable result is unchanged: `{"score","via","record"}`. A working result is `{"score","via","tier":"working","entry":{...}}` with the entry's fields of section 11.2 (never a `record`). The history channel (section 11.7) is `"tier": "history"`.
- **Flags:** `--no-working` leaves the working tier out; `--working` returns only working entries; `--no-history` and `--history` do the same for the history channel. `--text-only` leaves out meaning for all tiers. Without any working entry and without a history source, `find` is byte-for-byte CodeLore's.
- **Embeddings:** working entries are searched by words and terms only; they are not embedded (they are short-lived, and a vector costs more than the entry is worth).
- **Who sees what:** a member sees its team's pulses, the messages addressed to it, to `team` or sent by it, and the briefs of its team's tasks; the lead sees all of its team's; the owner and the desktop see every team's. A search from outside a team (the owner's `hgs lore find --project`) sees all. No search crosses to another project.
- **Counting toward `-n`:** working and history results share the limit; they are ranked lower, so they fill the tail. At most `ceil(n / 2)` of the returned results are working, and at most 3 are history, so a few durable records are never pushed out by chatter.
- **`for <file>` and `show`** read the durable tier only, and `show <w-id>` shows a working entry by id (until it expires).

### 11.7 Verbatim history: federated, never copied

Storing verbatim text is cheap; reading it is what costs. So lore does not copy transcripts into its store and has no index of them. It asks Zerus's existing local content-search index over the sessions' histories (private, bounded: `hgs search --indexed`, `content-search-query-v1`, a page of at most 100 verified hits and 2 MiB, every returned session proved by a fresh binding) through a provider.
- **The provider trait.** The `codelore` crate defines `HistorySource` (a `Source` plug-in, section 11.14): `search(query, scope, limit) -> Vec<HistoryHit>`, where a `HistoryHit` is `{session, run_id, conversation_id, message_id, role, at, excerpt, score}`. Zerus implements it over the content-search index and registers it; standalone `lore` has none and says "no history source" once in `notes`.
- **What `find` does with it:** it asks for at most 10 hits, takes the 3 best by the provider's score, ranks them below the working tier (their fused score multiplied by 0.25), and returns them as `"tier": "history"` with `{session, message_id, role, at, excerpt}`. A provider that is slow (over 500 ms) or unavailable adds one note and the search goes on without it; it never blocks a hand-out (section 6.2).
- **Retention.** Lore keeps nothing. A history hit lives as long as the session's history and the index entry do: Zerus's own retention for histories decides, and a forgotten or changed session retires before any hit is exposed (the index's own rule). A hit that a person promotes (section 11.8) is the only way text of it persists in lore.
- **Size bounds.** The excerpt is at most 400 characters, centred on the match; the history channel of one `find` is at most 8 KiB; no lore file holds a hit.
- **Secret redaction.** Every excerpt passes the secret scan's patterns (section 11.9) **before it leaves the provider**; a match is replaced by `[redacted: <pattern name>]`, so a secret that is in a transcript is never copied into a result. Redaction is best-effort: it cannot find a secret that fits no pattern, and terminal output (screens, command output, build logs) is scrubbed the same best-effort way; the docs and the guide say so. Hard rule 1 holds either way: the excerpt is not stored.
- **Who may search whose history.** A member's `find` reaches the history of its own sessions only (its own earlier context, for example after a compaction). The lead's reaches its team's sessions (the lead's and its members'). The owner's, from the desktop, reaches any session, as the desktop search does. History of another project or of a session outside the caller's reach never appears.
- **Per-session opt-out.** A session has a setting `memory_history` (default on) in its options, in Qt and Zerus Next, and a project default. A session that is off is never returned by the history channel for anyone but the owner's own desktop search; a team session can be created off. The setting changes only what lore federates: it does not change the content-search index, and the owner's own desktop search is the owner's. Turning it on or off applies at once and is journaled (`session_history_memory`).

### 11.8 Promote

`lore promote <id> [--kind K] [--field name=value]… [--dry-run] [--by NAME]` turns a working entry (or a history hit, `lore promote history:<run>/<message>`) into a **durable record under the strict rules**. It is the only way working text becomes durable, and the only way a history excerpt does.
- **Draft.** The entry prefills a draft; the caller fills the rest. `--kind` is required unless the entry's kind fixes it:
  - a **brief** becomes a `topic` (`title` from the task's title, `body` the brief's text); the topic's `body` ceiling is 3,000 characters, so a longer brief is refused with "too long for a topic" and never cut;
  - a **message** becomes a `decision` (`question` = its first line, `choice` and `reason` from `--field`, `decided_by` the sender or `--field`) or a `topic`, by `--kind`;
  - a **pulse's** `blocked` becomes a `fault`'s `symptom`, and `done`/`next` a `reading` or `topic`'s text, by `--kind`; `cause`, `fix` and `check` come from `--field` (a fault needs all four, as section 6.1 says);
  - a **history excerpt** (at most 400 characters, already redacted) goes into the record's text as a quotation with its source named in the words (`session <name>, message <id>`): the format has no new key for it.
- **The write is the durable write:** `validate()`, the duplicate check (section 11.10), the secret scan on every field, the writer lock, one commit, with the trailer `Zerus-Promoted: <entry id>`. A refusal writes nothing and names the field: the secret scan's, the schema's, the duplicate's.
- **After it:** the working entry gets `promoted = <the record's id>` and keeps its normal expiry; promoting it again answers the existing record id (the request id is the entry's id), and writes nothing.
- **Who:** the lead and the owner may promote any entry of their team (the owner: any); a member may promote its own entries into its team's workspace. Into the project store (not a team workspace) only the owner's export or the project rule's (section 4.5) moves records, as ever.
- **`--dry-run`** prints the draft, the duplicate candidates and every refusal, and writes nothing.
- **In `plain` and `off`:** not available (section 11.5).

### 11.9 The secret scan

Hard rule 2. Before the commit of **every durable write, every promote and every automatic Teams write**, every string field of the record (and every field of the draft a promote builds) is scanned. The write is refused and nothing is written if a field matches:
- **Key and token patterns** (case-sensitive unless noted): a PEM private-key block (`-----BEGIN [A-Z ]*PRIVATE KEY-----`); AWS access key ids (`AKIA`/`ASIA` + 16 upper-case letters or digits); GitHub tokens (`gh[pousr]_` + 36 or more); Slack tokens (`xox[abprs]-`); Hugging Face (`hf_` + 30 or more); OpenAI-style and Anthropic keys (`sk-` or `sk-ant-` + 20 or more); Google API keys (`AIza` + 35); a JSON Web Token (three base64url parts, the first two starting `eyJ`); credentials in a URL (`://user:password@`); and an assignment to a name that ends in a word that says it is secret (`password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, case-insensitive, so `DB_PASSWORD`, `access_token`, `client_secret` and a quoted JSON `"password"` count), followed by `=` or `:` and 8 or more non-space characters that are not a placeholder such as `<…>`, `***`, a run of `xxxx`, or `${…}`.
- **High-entropy strings:** a token of 24 or more characters from `A-Za-z0-9+/_=-` whose Shannon entropy is at least 4.0 bits per character. Exempt: a hex string of 7 to 64 characters (a commit or hash, in any field), a UUID, and anything containing a `/` or `.` that reads as a path or name (`src/render/post.py`, `scene.depth_buffer`).
- **The refusal names the match, never repeats it:** "refused: `fix` holds what looks like an AWS access key id (`AKIA…WXYZ`, 20 characters); nothing was written", showing at most the first 4 and last 4 characters and the field. `--json` gives `{"error", "details": [{"field","pattern","length","masked"}]}`.
- **A known false positive** is cleared by an exact string in `lore.toml`'s `[secrets] allow = [...]` (the owner's edit; each entry is a literal, never a pattern). An allowed string is still redacted in history excerpts.
- **Teams' automatic writes** that are refused are a journal event `lore_refused` with the pattern name and the field (never the match) and a lead inbox item; the record is not retried until a field changes.
- **Working-tier entries are redacted, not refused:** a pulse, message or brief that matches is stored with each match replaced by `[redacted: <pattern name>]` and the write answers with a note saying so. The bus must not lose a message, and a working entry is never durable; the durable brief (section 11.12) and any promote still refuse.
- **Best-effort, said plainly:** the scan finds the patterns above and high-entropy strings; it does not prove a text has no secret. The guide says it and says that scrubbing terminal output, screens and logs is best-effort too.

### 11.10 Duplicates and conflicts

Two members writing the same fault is the normal case in a team.
- **The duplicate check returns the candidates** (section 1.1: signals, cosine and word overlap, the `names differ` downgrade), up to 3, blocking and near, strongest first. It runs **inside the writer lock** (section 2.3), after the previous writer's commit, so two concurrent writers cannot both miss each other: the second sees the first. A refusal still names the candidates.
- **When a blocking candidate is the same fault** (the new record's `cause` and `fix` words overlap the candidate's at the `dedupe.jaccard` threshold, 0.5, or the cosine threshold, or a signal is shared) the writer **adds an occurrence** instead of refusing: the candidate gets one line in its optional field `seen` (a list of at most 20 strings of at most 160 characters, `<date> <by> <where>`, oldest dropped), its `updated` moves, and nothing else changes. One commit. The new record is not written. The write answers `{"occurrence_of": "<id>"}`.
- **When it differs** (the candidate is blocking but `cause` or `fix` disagree) the new record is written as its own record and the writer names the conflict: it answers with `conflicts_with: ["<id>"]` and, in a team, Zerus puts one lead inbox item ("two faults disagree: `<a>` and `<b>`: supersede, keep both, or merge") with the ready commands. **The writer supersedes only on an explicit request:** `lore add … --supersede <id>` or `lore supersede <old> <new>`, which in one commit sets the old record to `superseded` with `superseded_by` (CodeLore's existing lifecycle) and writes the new one; the lead or the owner decides, never an automatic write.
- **Format note.** `seen` is the one addition to the record format that this addendum needs: an optional list of strings on the five knowledge kinds. `FORMAT.md` section 7 says a change that only adds an optional field keeps the version, and section 3 says unknown fields are kept and round-tripped, so a CodeLore that does not know `seen` keeps it on rewrite and the format stays version 3. `FORMAT.md` documents the field and the conformance run checks the round trip (section 8, point 8). Nothing else in section 11 touches the format.
- **In a synced store** the replica's conflict rules of section 3.5 apply unchanged: `seen` is an append-only list, so concurrent occurrences merge as a union (capped at 20 after the merge); the same `(kind, id)` with different bytes is the owner's Waiting item; a duplicate found only at merge is a conflict item (keep both, skip, or supersede the older), never a silent drop; supersede by two records keeps both relations.

### 11.11 Messages: urgency and the doorbell

A message is delivered by a **doorbell**: a short notice, never the message text. The text stays in the store and the recipient reads it with `lore messages --unread` (`hgs lore messages --unread`, or a member's `hgs team lore messages`).
- **The doorbell text** is one line: `[Zerus lore · message · from <by> · normal|URGENT · <id>] read it with hgs lore messages --unread`, at most 200 characters. It is the only text pasted, and an urgent one is marked `URGENT` in it, in the lead's inbox item and in the desktop's session row.
- **Normal:** rung at the recipient's **next idle moment** (the session is not working and its composer is empty), coalesced: at most one doorbell per recipient per 5 minutes, naming the count ("3 messages") when several wait.
- **Urgent rings at once, at the next safe point.** A **safe point** is the first of: the session is idle (ring now); a **tool boundary** where the agent has a hook for it (a Claude Code `PostToolUse` hook, a Codex or Kimi equivalent as their native contracts allow), where the doorbell is returned as the hook's `additionalContext` and never typed; the **turn end**. `safe_point` records which. Task 277 (owner, 2026-11-08): when the recipient is working and no hook boundary comes, Zerus **interrupts the turn natively** through `hgs interrupt` (Claude, Codex and Kimi stop; DeepSeek aborts; OpenCode stops), pinned to the displayed turn, waits for the stop and opens a new turn that carries the message on top and `Continue your task from where you stopped.`; the attempt goes on. Claude and Codex get the text from the turn hook, the others in the prompt.
- **Never into a busy composer.** A working session's composer is never written to. The interrupt is a native stop of the turn, not a paste; if the agent has no interrupt (an old or unsupported native backend), the urgent message waits for the turn end and the alarm below names it.
- **When an urgent message stays unread** for `urgent_alarm_minutes` (team setting, default 3, 0 turns it off) after the new turn, or Zerus could not interrupt the turn, the lead gets one urgent `urgent_unread` inbox item naming the member, the sender and why (waiting for the stop, delivered but not acted on, could not interrupt, or waiting for the turn end). For a message to the lead, the owner gets the Waiting item instead, and Zerus does not interrupt the lead. Ordinary unread notices are folded into the single "does not read" item and are never shouted. (The earlier `urgent_bound` of 10 minutes is kept as the ring bound for a message that never got a safe point; the alarm is the owner's shorter bound.)
- **A hand-out the member took is not unread.** A hand-out or stage message whose text reached the member's turn (`injected_at`) for an attempt it holds is marked read by the worker (`read_via: taken`); "does not read" counts only messages that never reached it.
- **A message to the owner** is an inbox item and a Waiting-for-you card (the existing channel), marked urgent when it is; the owner's reply is a message to the sender.
- **Read marks** are set by `lore message read <id>` and by reading it with `lore messages` (a `--peek` does not mark). A read marks only the reader's own copy: a message to it, its own mark on a `team` message (read for everyone once every named recipient has read it), `lead` for the lead, `owner` for the owner; `message read` of another's message is refused. The reader is the verified caller: the viewer the embedding pins (`LORE_CALLER`, `member:<team>:<name>`, `lead:<team>:<name>` or `owner`), a member (or a lead it names) in an agent session without one, the owner at a terminal; `--role` only narrows it, and `--by` or `--team` that differ from a pinned caller are refused. A normal message that stays unread past the next phase of the team (or 24 hours, whichever is first) is mentioned once in the lead's inbox as a stall (the recipient does not read), and the unread cap of section 11.4 backs it.

### 11.12 Briefs survive a restart

- **The latest brief per task is a durable artefact,** so a restart of the home computer, a lost `working.sqlite` or a compaction never loses it. It is a committed file, not a record (the topic kind's 3,000-character ceiling and its two statuses do not fit a brief, and a brief per task would fill the durable lore):
  - **With a store** (`team` and `project`): `<workspace>/briefs/t<task>.md` in the team's workspace (section 4.3), outside `lore/`, so `lore check`, the index and the Loro replica never see it. Its first line is `# Brief: <task title>`, then the text (at most 8 KiB). A new version rewrites the file in one commit by the writer, under the writer lock, with the trailer `Zerus-Brief: <team>/t<task>/v<version>`; the older versions are in git. The text passes the secret scan first: a brief that matches is refused with the match named, the lead is told to rewrite it, and the file and the working row keep the previous version. (A `read-only` team writes nothing durable: its briefs stay working rows and its lead is told so.)
  - **In `plain`:** a row in `teams.sqlite3` (`team_briefs`), latest per task, kept as long as the team record is (section 5).
  - **In `off`:** the brief is Zerus's present dispatcher text, as today.
- **The working rows are the history:** every version is a working row; the TTL of section 11.3 governs the older versions only.
- **When the task closes,** the file stays for 30 days, then the writer removes it in one commit (git keeps every version: `git log -- briefs/t<task>.md`). The closing is the task's own (accepted, cancelled, or the team's finish) and a reopen cancels the removal.
- **Reading:** `lore brief show --task REF` answers the latest from the file (the working row if the workspace is unreachable); `--version N` reads a working row while it exists, or the file's git history.
- **Not searched after the task closes:** a closed task's brief is not a record, so `find` does not return it; `lore brief show` does. Promote it (section 11.8) to keep it as a topic.

### 11.13 The interface

The interface is the CLI, `lore` and `hgs lore`, with `--json`. No MCP server is part of this design. An MCP wrapper, if ever wanted, calls the CLI's `--json` and changes no format; the JSON shapes below are additive-only (a key may be added, never removed or renamed) and are the contract.

| Command | Does |
|---|---|
| `lore pulse add [--task REF] [--done T] [--next T] [--blocked T] [--by NAME]` | writes a pulse (section 11.2); `--json` answers the entry |
| `lore pulses [--task REF] [--by NAME] [--since TIME] [--all] [--json]` | the current pulse per member and task, or with `--all` every unexpired row, newest first |
| `lore message send --to WHO [--urgent] TEXT` (or stdin) | writes a message and rings by section 11.11; answers `{id, state, urgency}` |
| `lore messages [--unread] [--to WHO] [--from WHO] [--peek] [--json]` | lists and, unless `--peek`, marks read |
| `lore message read ID` | marks one read |
| `lore brief set --task REF` (text on stdin), `lore brief show --task REF [--version N]` | section 11.12 |
| `lore promote ID …` | section 11.8 |
| `lore supersede OLD NEW` | section 11.10 |
| `lore working status [--json]` | counts by kind, the bytes, the oldest entry, the last cleanup and whether a history source is registered |
| `lore working cleanup [--json]` | one cleanup pass now (the same bounds) |
| `lore find … [--working \| --no-working] [--history \| --no-history]` | section 11.6 |

- **In Zerus:** `hgs lore …` is these commands with Zerus's addressing (`--project`, `--team`); `hgs team lore plans|pulses|messages|brief` read the team's own working tier and the workspace. **`hgs team lore plans`** is `lore plans` over the team's workspace, plus for each live item the current pulse of the member that holds it: it is the answer the lead needs after a compaction (section 11.15, test 3). Members use `hgs lore message send`, `hgs lore pulse add` and `hgs lore brief show`; the hand-out names them in one line (section 6.2).
- **JSON entry shape:** `{"id","kind","team","by","at","expires","promoted", …the kind's fields}`; a list is `{"entries":[…], "notes":[…]}`; a refusal is `{"error","details"}` as CodeLore's.

### 11.14 The lore and Zerus boundary

- **CodeLore never depends on Zerus** (section 2.1). The working tier is in the crate: the tables, the kinds, the expiry and bounds, the cleanup, the commands, the secret scan, the promote and the duplicate rules; they run standalone with no team and no history source.
- **What Zerus supplies** through traits the crate defines: `HistorySource` (section 11.7: the content-search index), `Doorbell` (`ring(recipient, text, urgency) -> Delivered{safe_point}`: the delivery of section 11.11, which only Zerus can do, because only it knows a session's state and hooks), and `TaskClock` (`closed_at(task) -> Option<time>`, `open(task) -> bool`: how expiry learns a task's life). Standalone `lore` has no doorbell (messages stay unread until read, and urgent ones are only marked), no history (a note) and a task clock that reads the task from `lore plans` items (`#N` is the plan item id).
- **Zerus implements and registers them** in `hgs lore` and the dispatcher; the registration is the only place the two meet, so the crate's own tests run with recorded fakes.

### 11.15 Acceptance tests

They run in the section 8 style: fixtures, key-based grading (a control question is answered when the answer holds one of its keys, as the retrieval bench judges), no model as judge. Each fixture states its N and its bar.
1. **A brief is enough (N = 10).** For each of 5 recorded tasks (a rework, a multi-file feature, a task with a decision behind it, a test-only task, one with a constraint that must not be broken) a fresh session is given **only the brief** and answers 10 control questions about the task (goal, files, constraints, what is decided and why, what is out of scope, the acceptance, the first step) **without asking the owner and without a tool call to anything but `hgs lore brief show`**. The bar: at least 9 of 10 per task, and no answer that states a fact the brief does not hold (counted as wrong).
2. **A week of pulses reconstructs the week (N = 10).** A recorded team week (the fixture journal of a real team, 5 days, 4 members, about 40 tasks) is turned into its pulses by the section 11.2 rules. A reader given **only the pulses** answers 10 control questions (who did what and when, which tasks were blocked and on what, what was reworked, what finished last). The bar: at least 8 of 10, with every `done` and `blocked` of the questions' keys present in the pulses.
3. **The lead after a forced compaction (N = 12).** A team fixture is run to a forced compaction (section 6.2's recall trigger, source `compact`); the lead, given **only `hgs team lore plans` and the pulses**, answers 12 control questions about the team's state (who does what, what was decided, what waits for whom, what is blocked, what is next) **without asking the owner**. The bar: at least 11 of 12 and zero invented facts.
4. **Expiry (section 11.3), exactly:** a pulse of an open task never expires; one of a closed task expires at close + `pulse_days` and not a second before; a reopen clears it; a read message expires at `read_at` + `message_days`; an unread one at `unread_days`, with its `working_unread_expired` event and inbox item; old brief versions at the task's close; the latest brief never. Run with a fake clock.
5. **Bounds (section 11.4):** each row of the table at its bound (the refusal text, the order of what the cleanup deletes, the protected entries); the cleanup deletes at most 500 rows and holds no write transaction over 50 ms; durable writes never wait for it.
6. **Search (section 11.6):** a working entry never outranks a durable record that matches by signal or phrase; the half-and-3 caps; the marks and the JSON shape; without working entries and without a history source `find` is byte-for-byte CodeLore's (the conformance of section 8, points 1 to 3, must still hold: the live two-CLI run 13 of 13 and the 300-invocation script equal).
7. **Promote (section 11.8):** each mapping, the refusal paths (secret, schema, duplicate), idempotence by entry id, `--dry-run` writing nothing, `plain` unavailable.
8. **Secrets (section 11.9):** a corpus of one sample per pattern and of near misses (a git sha, a UUID, a path, a placeholder) is refused or passes as stated, with the match masked in the refusal; history excerpts are redacted before leaving the provider; working text is redacted, not refused; verbatim text never appears in `git log -p`, in the store's tree or in a Loro snapshot, apart from the authored brief files and the promoted records, which passed the scan (a test greps all three after a run that wrote every kind).
9. **Duplicates and conflicts (section 11.10):** two writers racing the same fault leave one record with one occurrence, not two records; a differing fault is written with `conflicts_with` and the inbox item; supersede only on request; `seen` merges as a union in two replicas; the round trip (Python 0.3.0 rewriting a file with `seen` keeps the field).
10. **Urgent delivery (section 11.11):** a busy session's composer receives nothing (a recorded session in each of: idle, working with a hook, working without one); the hook returns the doorbell as context; the turn-end delivery; the bound, the lead's inbox item and the native stop (the stop is requested through the existing action, never a paste); `URGENT` in the three places.
11. **History (section 11.7):** the provider fake, the 3-hit and 8 KiB bounds, the scope per caller (member, lead, owner), the per-session opt-out, the unavailable-source note, and that nothing the provider returned is in any lore file.
12. **Briefs survive (section 11.12):** delete `working.sqlite` and restart: the latest brief of each task is still there, from `briefs/t<task>.md` or `teams.sqlite3`; a brief with a secret is refused with the previous file intact; the file is removed 30 days after the close and a reopen cancels it; `git ls-files` shows no brief outside `briefs/`, and no brief is in the index or a Loro snapshot.

### 11.16 Decisions this addendum takes, for the owner to confirm

The owner's points and the two reviews fix the shape; these numbers and choices are this document's, and each is a setting or a one-line change:
- the defaults: `pulse_days = 14`, `message_days = 7`, `unread_days = 30`, `urgent_bound = 10 min`, the bounds table, the 0.5 and 0.25 rank weights and the caps of half and 3;
- **the working tier is not replicated** in a synced project (section 11.5), because replicating expiring rows through a CRDT would make expiry race the merge, and the home-computer rule already carries the cross-computer path;
- **working text is redacted, not refused,** on a secret match (section 11.9), so a message is never lost to a false positive;
- **who may search whose history** (section 11.7): members their own, the lead its team's, the owner any;
- **`seen`,** the one format addition (section 11.10), an optional list on the knowledge kinds, which keeps format version 3;
- **a brief is a committed markdown file** in the team's workspace, not a record, removed 30 days after its task closes (section 11.12).

### 11.17 Out of scope

- An MCP server (section 11.13).
- Copying, exporting or indexing transcripts in lore, and anything that puts verbatim history into git or the replica (hard rule 1).
- Semantic (embedding) search of working entries and of history.
- A working tier shared across projects, or a working tier on a computer that holds no store.
- Any delivery to a working composer, by any mechanism (hard rule 3).

