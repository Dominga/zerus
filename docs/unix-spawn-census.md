# Unix helper spawn and descriptor census

Snapshot for zerus-0w3zj.9 slice 1, 2026-11-14. This covers the production
Rust module graph of `src/main.rs`, `client/core/src/lib.rs` and
`client/src-tauri/src/lib.rs` for the union of Linux and macOS, including
optional features and path-shared modules. Test-only and Windows-only source,
thread/task creation, domain methods named `spawn`/`status`/`bind`, and HTTP
response builders are excluded. Each row identifies a **source call site**,
not its dynamic frequency. Source line numbers are snapshot locators; the
function names remain useful as source moves.

The review combined a Rust syntax-tree scan of reachable modules with `rg`
for process, socket, pipe, descriptor duplication, and pre-exec APIs. The
scope excludes other executable entrypoints (including the mobile connector's
separate Tokio gate), dependency implementation details and macro expansions.
Framework-created descriptors and external implementations of injected spawn
callbacks remain gaps, not evidence that all open descriptors are covered.

## Slice 1 contract

`zerus_client_core::unix_spawn` owns one parent-side mutex, an opaque `Turn`,
and CLOEXEC endpoint factories. Root `platform::spawn_turn` delegates to that
same mutex. Tauri's path-shared `child_environment` references the **linked**
core module, so it cannot compile another mutex. Non-Unix spawn adapters call
std unchanged; Windows keeps its pipes and existing flags/guards.

A turn covers endpoint creation/flag setup and the actual spawn. An existing
turn is passed to `_in_turn` factories instead of recursively locking. A turn
ends before post-spawn verification, writer/readers, child wait, drain or join.
The deadline-aware root paths retain their existing checks and try-lock loop.
Prepared helper stdin still attaches its original writer before any guard;
acquiring-PID shutdown ownership and every withhold predicate are unchanged.

This slice adds **no pre-exec hook, registry, close plan or child fd closure**.
It does not change std's backend eligibility, pidfd file actions or forkpty's
child setup. Existing pre-exec sites are inventoried below, not installed by
the shared module. POSIX spawn eligibility remains where it was; the timing
fixtures separately cover plain std, piped, existing pre-exec and forkpty paths.

Teams' reserved-turn Git path keeps its existing reservation before the store
write admission and its execution clock after reservation. The token type is
mechanically adapted at `src/teams/integrate.rs:669`; its reserve and consume
calls remain at lines 341 and 683. `platform::spawn_turn_before` retains main's
separate Deadline/Cancelled outcomes, try-lock loop and final pre-spawn checks.
`run_bounded_in_turn` consumes that token only for spawning and releases it
before the existing I/O/wait loop. It does not reacquire the gate.

## Declared gaps and follow-on boundaries

- Generic `platform::output/status` remain opaque synchronous calls. There are
  **7 direct wrapper callers: 3 output and 4 status**: `platform::capture`,
  `platform::konsole_get`, `state::processes::native::descendants`,
  `platform::silent`, `platform::konsole`, `accounts::install::run`, and
  `machines::setup`. Wrapper implementations are counted separately below;
  callers of `capture`/`silent` are transitive, not additional direct sites.
  No mutex is placed around an entire synchronous wait and no I/O is rewritten.
- All direct synchronous Command `output/status` primitives are listed as gaps
  below, including those wrappers, SSH, process inspection, tmux, native UI,
  clipboard and metrics. Their inherited-descriptor exposure is unchanged.
- General listener/connect/accept and the Tauri signal self-pipe remain gaps.
  Blocking connects/accepts cannot hold the spawn turn; signal installation's
  raw-fd lifetime and ignored fcntl results need a separate reviewed change.
  DBus/zbus, WebKit/Tauri, async I/O and other libraries own additional sockets,
  pipes and internal spawns outside this module's cooperation boundary.
- File/directory/image/custody/lock duplicates remain owned by their current
  domains. They are listed because future registration must follow their real
  ownership transfers; this slice adds no hidden duplicate or retirement rule.
  SCM_RIGHTS keeps the existing Linux atomic CLOEXEC and macOS guarded fcntl.
- std owns its internal stdio and exec-error descriptors; this module does not
  replace std's error protocol. `InputSpawn::spawn_with` and stream `Source`
  retain injected spawn callbacks. The in-tree root source uses
  `platform::spawn_service_child`; out-of-tree callbacks must cooperate.
- PTY participates in the parent turn but still uses `forkpty`. Master
  CLOEXEC is still set after forkpty, and libc still does its login_tty work
  before the existing child branch. That ordering gap belongs to
  zerus-0w3zj.10 (slice 3), which may use openpty plus explicit fork.
  Darwin's current dup-only fallback after login_tty failure remains intact;
  the bounded errno diagnostic is deferred to that explicit bootstrap.
- Slice 2 requires separate approval for a bounded endpoint registry, exact
  lifetime/dup/close tracking and first-hook closure before caller hooks.
  It must preserve intended image/custody fds, std error reporting, guards,
  deadlines, close-only custody, physical settlement and no-replay rules.

## Explicit spawn families

The tables include both primitive spawn sites and the adapters that call them;
these are **not additive counts of processes**. Existing raw `Command::spawn`
callers retain their current root turn. Default client helpers, stream
fallbacks, detached terminals, Tauri helpers and root direct spawns now join
that turn mechanically. Tauri's two Tokio service helpers use `platform::spawn_async`,
which holds the same core turn only across Tokio's synchronous spawn call;
their Tokio Child, async pipes and awaits remain intact. API v1 framing and call-site guard decisions do not
change.

### Command spawn primitives under the shared turn (12 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/unix_spawn.rs:51` | `spawn_in_turn` | `spawn` |
| `client/src-tauri/src/platform/mod.rs:38` | `spawn_async` | `spawn` |
| `src/desktop/original_child.rs:362` | `start` | `spawn` |
| `src/platform/unix.rs:251` | `spawn_in_turn` | `spawn` |
| `src/platform/unix.rs:268` | `spawn_prepared_in_turn_before` | `spawn` |
| `src/platform/unix.rs:1331` | `run_bounded_in_turn` | `spawn` |
| `src/state/claude_theme/helper.rs:101` | `run` | `spawn` |
| `src/state/effort/picker_scope/driver/read.rs:194` | `invoke_admitted` | `spawn` |
| `src/state/effort/picker_scope/driver/send_helpers.rs:93` | `discard` | `spawn` |
| `src/state/effort/picker_scope/driver/send_helpers.rs:199` | `invoke` | `spawn` |
| `src/state/effort/picker_scope/host.rs:564` | `cold_start` | `spawn` |
| `src/state/repositories/worker.rs:119` | `run` | `spawn` |

### Special backends under the shared turn (2 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/pty.rs:147` | `start` | `libc::forkpty` |
| `client/src-tauri/src/instance.rs:702` | `spawn_held` | `pidfd_spawnp-alias` |

### Mechanical spawn adapters (21 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/child_environment.rs:22` | `spawn` | `crate::unix_spawn::spawn` |
| `client/core/src/stream.rs:327` | `probe_until` | `crate::child_environment::spawn` |
| `client/core/src/stream.rs:1709` | `launch` | `crate::child_environment::spawn` |
| `client/core/src/terminal_launch/system.rs:95` | `start_detached` | `crate::child_environment::spawn` |
| `client/src-tauri/src/clipboard/linux.rs:137` | `wl_copy` | `crate::child_environment::spawn` |
| `client/src-tauri/src/desk.rs:1263` | `macos_tray_notify` | `crate::child_environment::spawn` |
| `client/src-tauri/src/desktop_service/catalog_relay.rs:172` | `start` | `crate::platform::spawn_async` |
| `client/src-tauri/src/desktop_service.rs:2240` | `connection` | `crate::platform::spawn_async` |
| `client/src-tauri/src/instance.rs:921` | `launchd_now` | `crate::child_environment::spawn` |
| `client/src-tauri/src/linux_accessibility.rs:50` | `start` | `crate::child_environment::spawn` |
| `client/src-tauri/src/settings.rs:110` | `open_url` | `crate::child_environment::spawn` |
| `client/src-tauri/src/updates.rs:1274` | `restart_helper` | `crate::child_environment::spawn` |
| `src/platform/unix.rs:109` | `spawn_ssh` | `zerus_client_core::unix_spawn::spawn` |
| `src/platform/unix.rs:230` | `spawn` | `zerus_client_core::unix_spawn::spawn` |
| `src/platform/unix.rs:234` | `spawn_group` | `zerus_client_core::unix_spawn::spawn` |
| `src/platform/unix.rs:1263` | `spawn_detached` | `zerus_client_core::unix_spawn::spawn` |
| `src/platform/unix.rs:1436` | `output_until` | `zerus_client_core::unix_spawn::spawn` |
| `src/platform/unix.rs:1695` | `wait_interactive` | `zerus_client_core::unix_spawn::spawn` |
| `src/state/backend/tmux.rs:633` | `stage` | `zerus_client_core::child_environment::spawn` |
| `src/state/claude_statusline.rs:343` | `compose` | `zerus_client_core::child_environment::spawn` |
| `src/state/supervisor.rs:24` | `spawn` | `zerus_client_core::child_environment::spawn` |

### Production helper-input adapter callers (16 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/run.rs:1237` | `spawn_helper` | `spawn` |
| `client/src-tauri/src/desktop_ownership/observation.rs:123` | `observe_current` | `spawn` |
| `client/src-tauri/src/desktop_ownership.rs:530` | `call` | `spawn` |
| `client/src-tauri/src/desktop_ownership.rs:823` | `execute` | `spawn` |
| `client/src-tauri/src/desktop_service/activation.rs:192` | `supported` | `spawn` |
| `client/src-tauri/src/desktop_service/activation.rs:242` | `read` | `spawn` |
| `client/src-tauri/src/desktop_service/creation_clone.rs:216` | `exchange` | `spawn` |
| `client/src-tauri/src/desktop_service/frontend.rs:683` | `owned` | `spawn` |
| `client/src-tauri/src/desktop_service/frontend.rs:824` | `execute` | `spawn` |
| `client/src-tauri/src/desktop_service/mode_transition.rs:724` | `exchange_bytes` | `spawn` |
| `client/src-tauri/src/desktop_service/notification_controls.rs:201` | `connect` | `spawn` |
| `client/src-tauri/src/desktop_service/notification_controls.rs:613` | `call` | `spawn` |
| `client/src-tauri/src/desktop_service/task_start_reconcile.rs:214` | `exchange` | `spawn` |
| `client/src-tauri/src/desktop_service/termination_outcome/programs/native.rs:166` | `spawn_with_custody` | `spawn` |
| `client/src-tauri/src/desktop_service/termination_outcome/programs/source.rs:263` | `spawn_program_child` | `spawn` |
| `client/src-tauri/src/desktop_service/termination_outcome.rs:621` | `spawn_input` | `spawn` |

### Input adapter callback boundaries (2 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/run/stdin.rs:61` | `spawn` | `spawn_with` |
| `client/core/src/run.rs:739` | `run_decoded` | `spawn_with` |

### Injected stream spawn boundaries (2 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/stream.rs:325` | `probe_until` | `spawn` |
| `client/core/src/stream.rs:1707` | `launch` | `spawn` |

### Direct synchronous spawn primitives: declared gaps (21 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/src-tauri/src/metrics.rs:538` | `sample_tree` | `output` |
| `src/native_ui.rs:108` | `address` | `status` |
| `src/native_ui.rs:127` | `address` | `status` |
| `src/native_ui.rs:172` | `open` | `status` |
| `src/platform/unix.rs:118` | `ssh_output` | `output` |
| `src/platform/unix.rs:121` | `ssh_status` | `status` |
| `src/platform/unix.rs:272` | `status` | `status` |
| `src/platform/unix.rs:276` | `output` | `output` |
| `src/platform/unix.rs:494` | `process_start_fallback` | `output` |
| `src/platform/unix.rs:641` | `process_table` | `output` |
| `src/platform/unix.rs:689` | `parent_and_argv` | `output` |
| `src/platform/unix.rs:709` | `parent_and_name` | `output` |
| `src/platform/unix.rs:1765` | `clip` | `status` |
| `src/platform/unix.rs:1789` | `clip_set` | `status` |
| `src/platform/unix.rs:1844` | `paste` | `status` |
| `src/state/backend/tmux.rs:35` | `tmux` | `output` |
| `src/state/backend/tmux.rs:55` | `picker_ps` | `output` |
| `src/state/backend/tmux.rs:711` | `kill` | `status` |
| `src/state/backend/tmux.rs:791` | `forward_key` | `status` |
| `src/state/dsh.rs:544` | `ensure_host` | `output` |
| `src/status.rs:109` | `desktop` | `status` |

### Direct generic platform output/status callers (7 sites)

| Source | Function | Wrapper |
| --- | --- | --- |
| `src/platform/mod.rs:99` | `capture` | `output` |
| `src/platform/mod.rs:479` | `konsole_get` | `output` |
| `src/state/processes/native.rs:284` | `descendants` | `output` |
| `src/platform/mod.rs:110` | `silent` | `status` |
| `src/platform/mod.rs:467` | `konsole` | `status` |
| `src/accounts/install.rs:187` | `run` | `status` |
| `src/machines.rs:504` | `setup` | `status` |

### Shared endpoint factories and callers (14 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/pty.rs:123` | `start` | `crate::unix_spawn::socket_pair_in_turn` |
| `client/core/src/run/stdin.rs:42` | `new_in_turn` | `crate::unix_spawn::socket_pair_in_turn` |
| `client/core/src/run/stdin.rs:45` | `new_in_turn` | `crate::unix_spawn::null_pair_in_turn` |
| `client/core/src/unix_spawn.rs:61` | `socket_pair_in_turn` | `UnixStream::pair` |
| `client/core/src/unix_spawn.rs:90` | `nonblocking_stream` | `libc::socket` |
| `client/core/src/unix_spawn.rs:113` | `pipe_in_turn` | `libc::pipe2` |
| `client/src-tauri/src/instance.rs:659` | `spawn_held` | `zerus_client_core::unix_spawn::pipe_in_turn` |
| `client/src-tauri/src/instance.rs:1509` | `listen` | `zerus_client_core::unix_spawn::socket_pair` |
| `src/desktop/notifications/host.rs:281` | `spawn` | `zerus_client_core::unix_spawn::socket_pair_in_turn` |
| `src/desktop/notifications/host.rs:283` | `spawn` | `zerus_client_core::unix_spawn::socket_pair_in_turn` |
| `src/desktop/notifications/host.rs:285` | `spawn` | `zerus_client_core::unix_spawn::socket_pair_in_turn` |
| `src/platform/unix.rs:290` | `nonblocking_socket` | `zerus_client_core::unix_spawn::nonblocking_stream` |
| `src/platform/worktree_membership/objects/loan/borrower.rs:33` | `socket` | `zerus_client_core::unix_spawn::nonblocking_stream` |
| `src/state/effort/picker_scope/host.rs:515` | `cold_start` | `zerus_client_core::unix_spawn::socket_pair_in_turn` |

### Other endpoint creation or acceptance: declared gaps (18 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/desktop/unix.rs:300` | `probe_request` | `Socket::new` |
| `client/src-tauri/src/instance.rs:1063` | `answers` | `socket2::Socket::new` |
| `client/src-tauri/src/instance.rs:1079` | `forward` | `socket2::Socket::new` |
| `client/src-tauri/src/instance.rs:1179` | `claim` | `UnixListener::bind` |
| `client/src-tauri/src/instance.rs:1538` | `listen` | `accept` |
| `client/src-tauri/src/instance_guard.rs:100` | `bind` | `UnixListener::bind` |
| `client/src-tauri/src/signals.rs:35` | `install_for` | `libc::pipe` |
| `crates/freedesktop-notify/src/ingress.rs:82` | `connect` | `async_io::Async::<UnixStream>::connect` |
| `src/desktop/mod.rs:706` | `serve` | `accept` |
| `src/desktop/transport.rs:141` | `bind` | `UnixListener::bind` |
| `src/desktop/transport.rs:147` | `accept` | `accept` |
| `src/desktop/transport.rs:169` | `connect` | `Stream::connect` |
| `src/desktop/transport.rs:201` | `connect_until` | `libc::socket` |
| `src/native_ui.rs:124` | `address` | `TcpListener::bind` |
| `src/speech.rs:418` | `exchange` | `UnixStream::connect` |
| `src/state/dsh.rs:537` | `ensure_host` | `UnixStream::connect` |
| `src/state/worktree_programs/holding.rs:140` | `serve` | `UnixListener::bind` |
| `src/state/worktree_programs/holding.rs:292` | `serve` | `accept` |

### Descriptor duplicates (including factory internals) (33 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/download_cache/maintenance.rs:392` | `read_index` | `try_clone` |
| `client/core/src/image/linux.rs:228` | `open` | `libc::fcntl` |
| `client/core/src/run/custody.rs:29` | `try_lock` | `libc::fcntl` |
| `client/core/src/run/stdin.rs:103` | `acquire` | `try_clone_to_owned` |
| `client/core/src/task_start/capture/journal/identity.rs:69` | `next_check_id` | `try_clone` |
| `client/core/src/task_start/capture/journal/publication.rs:141` | `writable` | `try_clone` |
| `client/core/src/unix_spawn.rs:70` | `null_pair_in_turn` | `try_clone` |
| `client/core/src/unix_spawn.rs:76` | `duplicate_in_turn` | `try_clone_to_owned` |
| `src/desktop/custody.rs:120` | `prepare` | `try_clone` |
| `src/desktop/feed/io.rs:140` | `proxy` | `try_clone` |
| `src/desktop/mod.rs:1820` | `client` | `try_clone` |
| `src/desktop/mod.rs:2139` | `watch` | `try_clone` |
| `src/desktop/notifications/host.rs:265` | `spawn` | `libc::fcntl` |
| `src/desktop/original_store.rs:178` | `sibling` | `try_clone` |
| `src/desktop/original_store.rs:214` | `walk` | `try_clone` |
| `src/desktop/original_store.rs:265` | `read` | `try_clone` |
| `src/desktop/task_message/runtime/reference.rs:279` | `poll_admitting` | `try_clone` |
| `src/desktop/task_message/staging.rs:866` | `entries` | `libc::dup` |
| `src/machines.rs:259` | `resolve_output` | `try_clone` |
| `src/machines.rs:260` | `resolve_output` | `try_clone` |
| `src/platform/unix.rs:1433` | `output_until` | `try_clone` |
| `src/platform/unix.rs:1434` | `output_until` | `try_clone` |
| `src/state/dsh.rs:579` | `ensure_host` | `try_clone` |
| `src/state/effort/picker_scope/driver/publication.rs:159` | `borrow` | `libc::fcntl` |
| `src/state/effort/picker_scope/driver/publication.rs:383` | `follow_own_settings_receipt` | `try_clone` |
| `src/state/effort/picker_scope/driver/publication.rs:472` | `commit_once` | `try_clone` |
| `src/state/effort/picker_scope/driver/publication.rs:476` | `commit_once` | `try_clone` |
| `src/state/guarded_recovery.rs:168` | `lock_file` | `try_clone` |
| `src/state/hooks.rs:1403` | `agent_version` | `try_clone` |
| `src/state/lifecycle.rs:40` | `agent_help` | `try_clone` |
| `src/state/terminating/writer_records/gate.rs:68` | `acquire` | `try_clone` |
| `src/state/worktree_cleanup.rs:2058` | `delete_tree` | `try_clone` |
| `src/state/worktree_cleanup.rs:2303` | `entries` | `libc::dup` |

### Existing descriptor transfer (1 site)

| Source | Function | Operation |
| --- | --- | --- |
| `src/platform/worktree_membership/objects/loan/transport.rs:243` | `part` | `libc::recvmsg` |

### Existing executable replacement (2 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/pty.rs:179` | `start` | `libc::execve` |
| `src/cli.rs:1296` | `run_base` | `exec` |

### Existing pre-exec registrations (unchanged) (12 sites)

| Source | Function | Operation |
| --- | --- | --- |
| `client/core/src/image/linux.rs:308` | `inherit` | `pre_exec` |
| `client/core/src/run/custody.rs:57` | `inherit` | `pre_exec` |
| `client/core/src/terminal_launch/system.rs:88` | `start_detached` | `pre_exec` |
| `client/src-tauri/src/clipboard/linux.rs:130` | `wl_copy` | `pre_exec` |
| `src/desktop/notifications/host.rs:306` | `spawn` | `pre_exec` |
| `src/phones.rs:451` | `descriptor_exec` | `pre_exec` |
| `src/platform/unix.rs:1256` | `spawn_detached` | `pre_exec` |
| `src/speech.rs:131` | `capture_command` | `pre_exec` |
| `src/state/effort/picker_scope/host.rs:553` | `cold_start` | `pre_exec` |
| `src/state/repositories/directory.rs:466` | `share` | `pre_exec` |
| `src/state/repositories/directory.rs:483` | `anchor` | `pre_exec` |
| `src/state/worktrees.rs:121` | `git_holding_cancellable` | `pre_exec` |


## Reproducing the spawn measurements

The ignored `desktop::original_child::spawn_cost::unix_spawn_cost` root test
measures 12 mechanisms; the ignored Tauri `instance::unix_spawn_cost_pidfd`
test adds Linux's pidfd-spawn family when glibc provides it. Both use only
synthetic `/usr/bin/true` and `/bin/cat` children, 8 warm-ups and 64 samples per
family, and report median, p95 and minimum microseconds. They start no agent,
GUI or owner session. Most timings stop at spawn return; the two explicitly
named round-trip families include their existing I/O/deadline/wait loop.

Run the same fixtures before and after in the same build profile with inherited
`HGS_*` cleared, a worktree-local Cargo target, the heavy wrapper and `-j 4`.
Use `ZERUS_SPAWN_COST_PHASE=before` or `after` to label the JSON lines. Run the
root test under `strace -f -e trace=process` separately from the untraced cost
samples to check existing `CLONE_VM|CLONE_VFORK` versus ordinary fork backends.
A busy shared host makes wall-time differences noisy; retain the unchanged std
Command control and raw logs, and do not infer backend changes from timings.

Tauri's ignored `platform::unix_spawn_cost_tokio` test additionally measures
the two async service helpers' backend, comparing the former raw Tokio spawn
call with the gated call in the same runtime and profile, 8 warm-ups and 64
samples each. It retains the same piped output and async wait in both cases.
