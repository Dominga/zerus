# Zerus Next (Tauri client)

The new desktop client of Zerus, growing beside the Qt GUI (`tray/`). Design:
`docs/superpowers/specs/2026-10-27-tauri-web-ui-design.md`; plan:
`docs/superpowers/plans/2026-10-28-tauri-client-phase1.md`. The approved visual
reference is `design/webui`; its components moved here and are bound to live data.

| Path | What it is |
| --- | --- |
| `src/` | SolidJS frontend: `bridge.ts` (per-frame batches), `store/` (catalog, Activity, interface state), `ui/` (rail, session list, Activity, styles), `skin/` (skin format v1 and the built-in skins) |
| `src-tauri/` | Tauri 2 app `zerus-client`: window creation, commands (`start`, `next_batch`, `follow`, `unfollow`, `metrics`), the change pump and the replay |
| `core/` | `zerus-client-core`: watch streams, catalog state and the coalescer, pure Rust |
| `fixtures/` | Synthetic replay streams (`scripts/make-fixture.py` writes them) |

The app is a separate application (`dev.zerus.next`, "Zerus Next (dev)"). It only
runs `hgs [@host] watch --json`, which is read-only; it never starts, stops or
replaces the installed Zerus GUI, CLI, agents or services.

## Build and run

```sh
cd client
npm ci
npm run check                  # TypeScript
npm test                       # Vitest: stores, transcript, screens
cargo +1.99.1 test --manifest-path src-tauri/Cargo.toml
RUSTUP_TOOLCHAIN=1.99.1 npx tauri dev
```

`npm run dev` alone serves the frontend in a browser; without Tauri it replays
`fixtures/replay-batches.json`, so the UI works without the app.

Environment:

| Variable | Effect |
| --- | --- |
| `ZERUS_CLIENT_HGS` | The `hgs` to run (default: the first `hgs` on `PATH`, then in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`, since an app started from Finder or a launcher gets a short `PATH`) |
| `ZERUS_CLIENT_REPLAY` | Replay synthetic watch streams instead of running `hgs`: a path list (`:` on Unix), the first file is this computer, `alias=file` entries are peers, e.g. `fixtures/replay.ndjson:mac=fixtures/replay-mac.ndjson` |
| `ZERUS_CLIENT_REPLAY_INTERVAL_MS` | The pause per `heartbeat` frame of a replay (default 900) |
| `ZERUS_CLIENT_QUERY` | URL parameters for the window, e.g. `select=claude/sample-project/api-refactor` opens that session and `page=settings` that screen |

A build without the `live` feature (`--no-default-features`) has no data source;
the frontend then uses its built-in replay.

## Headless runs

Development windows never open on a desktop someone is using. On Linux run the
app inside Xvfb and force X11: with `WAYLAND_DISPLAY` merely unset, GTK falls
back to the default Wayland socket (`wayland-0`) and the window opens on the
desktop. `ZERUS_CLIENT_HEADLESS=1` makes the app exit (code 3) instead of
opening a window anywhere but an X server display. WebKitGTK paints a black
window in Xvfb unless its DMA-BUF renderer is off:

```sh
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 xvfb-run -a -s "-screen 0 1440x900x24" \
  sh -c 'RUSTUP_TOOLCHAIN=1.99.1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson npx tauri dev'
```

## Actions

The session menus, the header and the Rename and Terminate dialogs run guarded
actions through the `hgs` CLI (`core/src/actions.rs`, `src-tauri/src/actions.rs`),
with Qt's enablement, confirmations and texts (`actions-ui.ts`). A peer's call
is prefixed with `@alias` and uses the longer timeout. Every call is made with
the child's own environment: the agent attribution (`HGS_SESSION`,
`HGS_RUN_ID`) is never passed on.

| Action | Arguments (`NAME` is the session name) | Standard input | Timeout here, peer |
| --- | --- | --- | --- |
| Stop (interrupt) | `interrupt NAME --json` | JSON: `request_id`, `expected_run_id`, `expected_conversation_id`, `expected_turn_started` | 15 s, 45 s |
| Mark as read | `read NAME --json` | JSON: `run_id`, `conversation_id`, `reply_id`, `attention_signature` | 15 s, 35 s |
| Pause | `pause NAME` | none | 45 s, 45 s |
| Resume | `resume NAME -d` | none | 30 s, 30 s |
| Rename | `rename NAME NEW [--archive ID]` | none | 5 s, 15 s |
| Move to archive | `archive NAME` | none | 30 s, 45 s |
| Terminate a tracked Codex, Claude or Kimi session | `terminate NAME --expected-run-id RUN` | none | 45 s, 60 s |
| Terminate any other live session | `kill NAME` | none | 5 s, 15 s |

Outcome rules:

- A call is made once and never repeated. A timeout, a failed hand-over or an
  abnormal end is an unknown outcome, shown as a failure and never replayed.
  Stop words them "Request timed out. Check Terminal before retrying." and
  "Transfer failed. Check Terminal before retrying."; the other actions show
  "timed out" or the trimmed standard error `hgs` printed.
- Stop succeeds only when the receipt echoes the request id, session name, run
  id and conversation id of the call (status `submitted`); anything else is
  "Invalid acknowledgement. Check Terminal before retrying." Its notices are
  Qt's: "Interrupt sent to NAME (COMPUTER). Waiting for the agent to stop…",
  "Stop NAME (COMPUTER): the turn already ended or is waiting for input. No
  action needed." when `hgs` answers "This turn is no longer working", and
  otherwise a sticky "Stop NAME (COMPUTER) failed: …" with "Open Terminal to
  check the agent before trying again."
- Pause, Resume, Rename and kill succeed on exit 0. Move to archive and
  terminate also read the single worktree report line on standard error: a
  removed worktree gives an information notice, a kept or unconfirmed one a
  sticky warning with its reason. Otherwise a failure shows the trimmed
  standard error.
- An entry acts on the row as it was when its menu, button or dialog was
  built. If the listed row has another run id by then (or, for a row without
  one, is gone), nothing runs and the notice says "This session changed. Open
  its actions again." Stop, Mark as read and terminate need the row's run id
  (Stop and Mark as read also its conversation id); the other actions name
  the session only.
- One write per session at a time. While it runs the session's entries show
  "Updating…"; another write for that session is a silent no-op (the Rust side
  answers `busy`). Stop and read marks each run in their own lane: Stop waits
  only for its own interrupt, as in Qt, and a read mark never holds up a write.
  Mark as read stays available while a write runs.
- Terminate stops a tracked Codex, Claude or Kimi session into Archive and
  kills any other live session; the confirmation says which. After the
  confirmation (and after a menu's Pause or Resume) the session must still be
  running, or still not running, as it was; otherwise nothing runs and the
  notice says "The session state changed. Open its actions again."
- Rename edits only the tag after the agent and project, with Qt's tag rule
  ("Enter a session name.", "Remove spaces at the beginning or end.", "Names
  cannot contain /, : or . or control characters.") and "This name is already
  in use on COMPUTER." for a live session of that name on the same computer.
- Stop is shown while the selected session's turn works (`working`, `tool` or
  `compacting`) and enabled when its computer is online and the details say
  `interrupt_supported`; it sends the details' run, conversation and
  `turn_started`. Hosts never list `interrupt` among the allowed actions.
- Details (allowed actions and their reasons, `auth_required`, Stop's fields)
  come from the followed session's Activity `state` value when it names the
  row's run and conversation. Only when the subscription answered without one
  does the client run one read-only `inspect`, refetched when the run,
  conversation or state changes, and never while the computer is offline.
- Mark as read sends the reply and attention the menu showed; if a newer reply
  or question arrived meanwhile, nothing runs and the notice says "A new reply
  or question arrived. Open the menu again to mark it as read." The row shows
  read once the host's snapshot reports it. The mark is saved before the call
  in `read-marks.json` in the app data directory (`app_data_dir`, e.g.
  `~/.local/share/<bundle id>/` on Linux; a replay uses
  `read-marks-replay.json`) so a restart keeps it. It is dropped once a row
  shows the host settled it, or when a complete snapshot of its computer no
  longer lists the session. While a mark is pending and a batch (an upsert or
  a snapshot) shows its row unread again for the same reply and attention
  signature, the call is retried quietly, at most once per 5 s per session
  (a person's Mark as read counts too). A host without shared read state keeps
  the entry disabled.

Forgetting, restoring, attention marks and sending arrive in later phases; their
entries are disabled with a reason. The copy entries below are live.

### Copy actions

The copy entries build text locally or ask `hgs` for it and put it on the
clipboard. They are not writes: they need no row lock and show no "Updating…".

| Entry | Text |
| --- | --- |
| Copy connect command | `hgs [@alias] a NAME` |
| Copy restore command (archived row) | `hgs [@alias] resume NAME --archive ID` |
| Copy session reference | the `zerus://session?` URI printed by `hgs [@alias] reference NAME --json --run RUN --conversation CONVERSATION [--archive ID]` (15 s here, 25 s on a peer) |
| Copy folder path | the `cwd` of the selected session's details, else the row's; disabled when empty |
| Copy session name | `NAME` |

- The commands are quoted with Qt's SessionCommands rules (POSIX shell; PowerShell
  on Windows) and name the `hgs` the client runs: `ZERUS_CLIENT_HGS`, else the
  one found on `PATH` or in the install folders.
- The reference takes the run and conversation of the selected session's
  details, else the row's. A reply must be a `zerus://session?` URI of at most
  8192 characters; otherwise the notice says "Could not read a session
  reference. Update hgs on this computer." Success shows "Session reference
  copied".
- The clipboard is written from Rust (tauri-plugin-clipboard-manager). A text
  that is empty or holds a line break is refused with Qt's texts. A failure
  shows "Zerus: could not copy" with the error. A success shows no notice,
  except for Copy session reference. The browser build (no Tauri) copies
  through `navigator.clipboard` with the same two checks.
- Each copy rechecks its row when it runs, as Qt does: a session that is gone
  or whose conversation changed copies nothing and shows "This session is no
  longer available. Open its actions again."; otherwise the current name,
  archive id, folder and run are copied.

### Actions smoke run

`ZERUS_CLIENT_QUERY=actions-smoke=1` (hook in `src/ui/PerfHost.tsx`, beside the
perf hooks) checks the actions end to end without the real `hgs`. It starts only
when the native side reports replay mode (`ZERUS_CLIENT_REPLAY`), an explicit
`ZERUS_CLIENT_HGS` and `ZERUS_CLIENT_PERF` (any value but `auto`; `exit_app` is
allowed only in a measurement run); otherwise it logs why and does nothing. In
replay mode `run_action`, `inspect_session` and `session_reference` refuse unless
`ZERUS_CLIENT_HGS` is set ("Actions in replay mode need ZERUS_CLIENT_HGS."), so
a replay never drives the real CLI.

Put `core/tests/support/fake_hgs_actions.py` as `hgs` in a scratch folder with a
`config.json` beside it containing
`{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=claude%2Fsample-project%2Freview"}}}`
(`reference` gets that URI; the write actions ignore standard output, and exit 0
is a success for them; `echo_interrupt` parses standard input and fails on
writes that have none). The fake logs each call to
`log.ndjson` beside it. Run it only with the headless environment:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_actions.py $S/hgs
echo '{"reply":{"exit":0,"stdout":{"uri":"zerus://session?machine=m&name=claude%2Fsample-project%2Freview"}}}' > $S/config.json
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_REPLAY=$PWD/fixtures/replay.ndjson \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=actions-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" src-tauri/target/release/zerus-client
cat $S/log.ndjson
```

The hook selects `claude/sample-project/review` (its details come from the
replay's Activity state value or, as here, one `inspect` of the fake), first
copies the session reference, the connect command, the session name and the
folder path through the store the menus use (the first calls `reference`; the
fake's URI is the clipboard text), logging each as
`actions-smoke copy-<entry>: done <the exact clipboard text>`, then runs pause, resume, rename (to `claude/sample-project/review-renamed`) and archive,
then the end call the Terminate dialog picks for the row through the dialog's
confirmation (`kill`, since the replay row is not tracked), then the other end
call (`terminate`), once each through the store the menus use. It logs each
outcome to stderr (`zerus-client perf: actions-smoke ...`) and exits 0, or 1
when an outcome failed.

## Screens

Overview, Projects, Machines and Accounts show live data and read only;
every write keeps its place, label and Qt enablement shape, disabled with the
phase that brings it ("Arrives with machine editing.", "... account
management.", "... project editing.", "... the New session dialog.", "... the
worktree flows.", "... the copy actions.").

| Data | Source |
| --- | --- |
| Machine cards (host, version, platform, metrics, console fields) | `catalog.meta`, projected in the core (`machine.rs`) and sent as `Batch.machines` |
| Connection state | `Batch.computers`: only `live` is fresh; `probing` and `reconnecting` connect (amber), `ended` is offline, `unsupported` reads "Update hgs" |
| Accounts | `hgs [@host] account ls` per host (every 30 s while Overview or Accounts shows, once per new host otherwise) and `account inspect ID --refresh` per installed profile (every 5 min, two at once, offline hosts skipped) |
| Machines | `hgs machine ls` on show and Refresh, `machine resolve ALIAS` on selection, `machine check ALIAS` only on Test connection |
| Projects | No read of its own: the organization the Sessions list groups by (the `organization` topic, or the core's `swarm get` poll for a CLI without it), this device's default project and the project sync status line |

The reads run in `src-tauri/src/screens.rs` on the blocking pool with the
core's timeouts and output limits (`core/src/reads.rs`); when each read is
due is decided in `core/src/views/schedule.rs`, and the core projects every
reply into a view model (`core/src/views`). A changed view emits
`screens-changed` with the screen's name, and the frontend pulls it again.
In replay mode the screens that read refuse unless `ZERUS_CLIENT_HGS` is set
("Reads in replay mode need ZERUS_CLIENT_HGS."); Projects keeps the project
sync status line. Peers without the event stream get no polling fallback yet
(see the phase table).

### Screens smoke run

`ZERUS_CLIENT_QUERY=screens-smoke=1` (hook in `src/ui/PerfHost.tsx`) shows
the four screens on the replay with a fake `hgs` answering the reads from
`core/tests/fixtures/views`, and exits 0 once each has loaded:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_reads.py $S/hgs; chmod +x $S/hgs
python3 - "$S" "$PWD/core/tests/fixtures/views" <<'EOF'
import json, sys
replies = {
    "account ls": {"file": "accounts-local.json"},
    "@mac account ls": {"file": "accounts-studio.json"},
    "account inspect claude-work --refresh": {"file": "inspect-claude-work.json"},
    "account inspect native-codex --refresh": {"file": "inspect-native-codex.json"},
    # The replay's other installed profiles have no fixture: their inspect fails.
    "account inspect native-claude --refresh": {"stderr": "no fixture", "exit": 1},
    "@mac account inspect claude-studio --refresh": {"stderr": "no fixture", "exit": 1},
    "@mac account inspect kimi-studio --refresh": {"stderr": "no fixture", "exit": 1},
    "machine ls": {"file": "machines.json"},
    "swarm get": {"file": "swarm.json"},
}
json.dump({"fixtures": sys.argv[2], "replies": replies}, open(sys.argv[1] + "/config.json", "w"))
EOF
cat > $S/run.sh <<'EOF'
src-tauri/target/release/zerus-client 2> "$S/app.log" &
app=$!
for page in overview machines accounts projects; do
  until grep -q "screens-smoke $page: shown" "$S/app.log" || ! kill -0 "$app" 2>/dev/null; do sleep 0.2; done
  sleep 1 # the frame after the page switch
  import -window root "$S/$page.png"
done
wait "$app"
EOF
env -u DISPLAY S="$S" WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 ZERUS_CLIENT_REPLAY="$PWD/fixtures/replay.ndjson:mac=$PWD/fixtures/replay-mac.ndjson" \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=screens-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" sh $S/run.sh
echo "exit $?"; grep screens-smoke $S/app.log
python3 -c 'import collections, json, sys; print(collections.Counter(" ".join(json.loads(l)["argv"]) for l in open(sys.argv[1])))' $S/log.ndjson
```

The screenshots stay in `$S`; they are never committed.

## Projects in the Sessions list

The Sessions list groups sessions by the shared project organization, as the
Qt GUI does, and its menus edit that organization (plan:
`docs/superpowers/plans/2026-10-28-tauri-client-organization.md`).

Data path:

- The source is this computer's `organization` topic (`core/src/organization.rs`);
  a CLI without the topic is polled with `hgs swarm get` every 5 s by the core,
  with the same states. Only this computer is followed: the catalog is shared.
- The organization hub (`src-tauri/src/organization.rs`) feeds each state to the
  write controller (`core/src/swarm/sync.rs`, a port of `SwarmController`) and
  hands the presented organization to the pump's grouping
  (`core/src/swarm/grouping.rs`).
- The pump puts `Batch.groups` (each row's project and rank, only for rows
  whose project or rank changed) and `Batch.grouping` (the headers and whether
  edits can run, when they changed) on its batches, and strips
  `Batch.organizations`: the organization itself never reaches the webview.
- `src/store/organization.ts` keeps the assignments and headers, and
  `src/ui/sessions/model.ts` groups rows by their ids and assignments only, so a
  status update never regroups. The header counts are computed there, beside the
  webview's state and machine filters.

Qt's grouping rules (`SessionOrganization.cpp`, `SessionsWindowRows.cpp`):

- Projects follow the shared `project_order`. Members are
  `organization.projects[].sessions[]`, keyed `<host>\n<session name>`, where the
  host is this computer's host name or the peer's alias (the row's `machine`). A
  session listed by two projects belongs to the first.
- A row no project lists belongs to the default project: this device's choice
  when it is listed, else the catalog's `default_project` when it is listed, else
  the first project, else the Inbox (`ungrouped`).
- With one project the list shows no header and nothing collapses. A project
  without visible members is hidden ("Hide projects without active sessions",
  on by default). A header's counts cover members the filters hide; muted
  members never count as attention or unread. Its colour is the project's
  `color`, else its palette slot.

This device's layout (collapsed projects, the order inside each project and the
default project) stays local, as in Qt. It lives in `organization-layout.json`
in the app data directory, remembers at most 100,000 members and is saved at
most once a second and as the app quits. Pending edits live beside it in
`organization-draft.json`, written through a temporary file and a rename
whenever they change, before their apply, and removed once they settle. A
replay keeps its own `organization-layout-replay.json` and
`organization-draft-replay.json`, and removes a leftover replay draft at start.

Edit commands (one `swarm apply` each, answered `done` with the conflicts it
recorded, `kept` with Qt's status text, or `refused` with the reason):

| Command | Menu entry |
| --- | --- |
| `move_to_project(members, project, before)` | Move to project, a drop |
| `create_project(name, members)` | New project… (in the Move to project dialog) |
| `rename_project(project, name)` | Rename project… |
| `move_project(project, before)` | Move project up, Move project down |
| `sort_project(project)` | Sort sessions by name |
| `delete_project(project, destination)` | Delete project… |
| `set_project_collapsed(project, collapsed)` | a header click, Collapse project, Expand project (layout only, no `hgs`) |

`move_to_project` takes a set of members, `<host>\n<session name>` from the
row's own machine (`memberOf`): every member goes into the project as one
`swarm apply` from one snapshot. The session menu passes one member; the batch
Move to project (`zerus-5wmh.20`) passes the selection. `before` places them in
this device's order only. Sort sessions by name makes every listed row an
explicit member; when they all are already, it changes only the layout and runs
no `hgs`.

Apply rules (`SwarmController.cpp`, ported in `core/src/swarm/sync.rs`):

- `hgs swarm apply` runs on this computer only, without `@host`, with
  `{base, desired, versions}` as compact JSON on standard input and a 10 s
  timeout; one runs at a time. Edits are sent 120 ms after the last one.
- Base and versions are the state the user saw, from one snapshot. A state is
  not adopted while edits are pending or an apply runs, and after an
  acknowledged write a state that does not show it yet is held back for 10 s.
  Only the fields an apply wrote (`written`) advance their versions, so another
  writer's change surfaces as a conflict, never as an overwrite.
- Nothing is sent before the first state. A catalog with `initialized: false`
  keeps the entries disabled ("Project sync is not set up on this computer yet.
  Start Zerus here once to import its projects."); this client never runs
  `swarm initialize`.
- A failed or timed-out apply keeps its edits: the list keeps showing them, the
  answer and the status line say "Project sync unavailable: <error>" (or "Project
  sync needs an updated HGS installation."), and the next tick sends one apply
  with the same base, desired state and versions. The tick is 5 s after the
  failure or the topic's next state, whichever comes first. Edits made meanwhile
  ride on it.
- After a restart the saved draft is shown at once and sent once after the first
  state, from its saved base and versions, so a catalog that changed meanwhile
  gives a conflict. A draft that cannot be used is moved aside, never applied,
  and the list says why until the first state.
- The status line keeps Qt's texts and reaches the Projects screen.
- In a replay, edits need `ZERUS_CLIENT_HGS` ("Actions in replay mode need
  ZERUS_CLIENT_HGS."); the browser replay refuses them ("Project changes need
  the desktop app.").

Deviations from Qt (the plan's Decisions):

1. The layout lives in `organization-layout.json`, not in QSettings
   (`workspace/organization`).
2. Sort sessions by name folds case, then compares as written, then by member;
   Qt uses `localeAwareCompare`.
3. No `swarm initialize`: a computer without the Qt GUI's import keeps edits
   disabled.
4. Edits made before the first ready catalog are refused, not queued.
5. Renames made while the client was closed are not followed yet (Qt follows
   them by persisted run tokens).

### Organization smoke run

`ZERUS_CLIENT_QUERY=organization-smoke=1` (hook in `src/ui/PerfHost.tsx`, with
the actions smoke's environment and guard) waits until the replay's organization
is ready and lists `claude/sample-project/review`, then runs one edit of each
menu through the store the menus use: Move to project (`p-aurora`), New project
("Smoke project", with the session), Rename project (`p-docs-site`), Sort
sessions by name (`p-sample-project`), Move project (`p-orders-api` before
`p-sample-project`) and Delete project (`p-docs-site` into the Inbox). It logs
`organization-smoke <step>: <outcome>` and exits 0 when every outcome is `done`.
The fake `hgs` answers `swarm get` and `swarm apply` with
`fixtures/replay-organization.json`, so each step's project is listed again
after every apply. The sort is layout only, so the log holds five `swarm apply`
calls and no `swarm get`, each from the fixture's organization (in its shared
form) and versions, and the layout file holds the sorted project. The screens
hub's account warm-up (`account ls`) reaches the fake too, which refuses it, so
the check counts only the `swarm` calls. `XDG_DATA_HOME` keeps the app data
directory in the scratch folder:

```sh
S=$(mktemp -d); cp core/tests/support/fake_hgs_swarm.py $S/hgs; chmod +x $S/hgs
python3 -c 'import json, sys; o = json.load(open(sys.argv[1])); json.dump({"get": [{"stdout": o}], "apply": [{"stdout": o}]}, open(sys.argv[2], "w"))' \
  fixtures/replay-organization.json $S/config.json
env -u DISPLAY WAYLAND_DISPLAY=zerus-none GDK_BACKEND=x11 ZERUS_CLIENT_HEADLESS=1 \
  WEBKIT_DISABLE_DMABUF_RENDERER=1 XDG_DATA_HOME=$S/data \
  ZERUS_CLIENT_REPLAY="$PWD/fixtures/replay.ndjson:mac=$PWD/fixtures/replay-mac.ndjson" \
  ZERUS_CLIENT_HGS=$S/hgs ZERUS_CLIENT_QUERY=organization-smoke=1 ZERUS_CLIENT_PERF=smoke \
  xvfb-run -a -s "-screen 0 1440x900x24" src-tauri/target/release/zerus-client 2> $S/app.log
echo "exit $?"; grep organization-smoke $S/app.log
python3 - "$S" fixtures/replay-organization.json <<'EOF'
import json, sys
S, fixture = sys.argv[1], json.load(open(sys.argv[2]))
calls = [c for c in map(json.loads, open(f"{S}/log.ndjson")) if c["argv"][:1] == ["swarm"]]
assert [c["argv"] for c in calls] == [["swarm", "apply"]] * 5, [c["argv"] for c in calls]
base = dict(fixture["organization"], session_assignments_explicit=True)
for call in calls:
    sent = json.loads(call["stdin"])
    assert sent["base"] == base and sent["versions"] == fixture["versions"]
layout = json.load(open(f"{S}/data/dev.zerus.next/organization-layout-replay.json"))
members = next(p for p in fixture["organization"]["projects"] if p["id"] == "p-sample-project")["sessions"]
title = lambda member: member.rsplit("/", 1)[-1]
assert layout["order"]["p-sample-project"] == sorted(members, key=lambda m: (title(m).casefold(), title(m), m))
print("5 swarm apply calls from the fixture's base and versions; p-sample-project sorted in the layout")
EOF
```

### Organization measurement

`src/store/organization.perf.test.ts` measures 20,000 rows: one replace into a
catalog (phase 2's reconcile), then 20,000 assignments in 50 projects with the
grouped members and list rows, then 1,000 status upserts, which must not group
the members again. It is skipped unless `ZERUS_CLIENT_MEASURE=1`:

```sh
ZERUS_CLIENT_MEASURE=1 NODE_OPTIONS=--expose-gc npx vitest run src/store/organization.perf.test.ts
```

The Rust grouping's own pass over 20,000 rows has an ignored timing test:
`cargo +1.99.1 test --manifest-path core/Cargo.toml --lib twenty_thousand_sessions_group_within_five_seconds -- --ignored --nocapture`.

## Measurement run

`ZERUS_CLIENT_PERF=auto` runs the Diagnostics scenarios unattended (`src/perf`),
writes the report to `ZERUS_CLIENT_PERF_OUT` and exits; every stage goes to
stderr as `zerus-client perf: ...` (no session names). It picks a session whose
Activity has events and scrolls, bounds each scenario at 60 s and the run at
8 minutes, and ends with 10 s idle windows: as displayed, with Animate working
sessions off, with Reduce motion (both only tried, not stored), with the window
hidden and with it minimised (`perf_away`; a bare Xvfb has no window manager, so
there the window stays shown and the log says `minimized false`), each of the
last two followed by one as displayed again.
`ZERUS_CLIENT_PERF_LABEL` names the platform, `ZERUS_CLIENT_PERF_HZ`
overrides the display rate (`display_hz`), `ZERUS_CLIENT_PERF_REPRESENTATIVE=0`
marks Xvfb runs and `ZERUS_CLIENT_PERF_PREFERENCE=default` keeps WebKit's
near-60 fps preference on macOS and Linux (the control run). The page is cross-origin
isolated (COOP and COEP headers) for precise timers.

Screenshots of real sessions are never committed; the fixtures are synthetic
(example domains, `/home/example` paths).

## Data path

Rust reads every watch frame (`client/core`), keeps the catalog and the followed
Activity, and folds changes into one batch per display frame. The frontend calls
`next_batch` from `requestAnimationFrame` with one call in flight; the command
parks up to 250 ms while nothing is pending and answers with raw JSON bytes. A
batch is one reactive transaction: rows reconcile by `computer + "\n" + key`, so
unchanged rows keep their identity and DOM. Only the selected session's Activity
is followed (newest 100 events); scrolling to the top reads earlier events with
`after`.

While the window is hidden or minimised the pump asks for nothing and schedules
no frame; a batch that arrives meanwhile waits, the Rust side keeps folding
changes into the next one, and the first frame after the window returns applies
it. The Working clocks stop too, as Qt stops its timers when a window hides.

## Settings and motion

The Settings screen has, so far, the two motion settings of the Qt GUI with its
QSettings keys and defaults: Animate working sessions
(`workspace/animateActivity`, on) and Reduce motion (`workspace/reduceMotion`,
off). They are stored per user in the webview's local storage, one JSON object
under `zerus-client-settings` keyed by the QSettings names (`src/store/settings.ts`).
`src/ui/motion.ts` turns them into root attributes, and `src/ui/styles/motion.css`
lists every endless animation: Animate working sessions off stops the working
indicators (halos, the project count, status and typing dots), Reduce motion or
the system's reduced-motion preference stops all of them (also the Zerus mark,
shimmer and spinners) and makes entrances instant, and a hidden or minimised
window pauses everything. `src/ui/styles/motion.test.ts` checks that every
keyframe animates only transforms and opacity and that every endless animation
is in those lists.

## Rules

- The UI never separates values with dots (U+00B7, U+2022, U+2219, U+22C5,
  U+2027, U+2981, U+30FB): use layout, commas or line breaks.
- Motion uses transform and opacity only and honours Reduce motion, Animate
  working sessions and the system's reduced-motion preference (`motion.css`).
- `src/ui/lib/os-icons.ts` embeds two Tabler Icons outlines (MIT, see
  `licenses/`); `src/ui/lib/icons.generated.ts` is markup generated from
  `lucide-solid` (ISC).
