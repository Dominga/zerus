# Linux packages and source installation

The four AUR packages ship Zerus Next as `hgs-tray`, built from `client/` with
voice and round video. Source installs use Next by default too. The coordinator
and owner perform installed-machine acceptance; local checks use scratch
installations and private sessions.

## What the installer preserves

`packaging/desktop/install.sh` keeps the executable name `hgs-tray`, the application desktop
entry, and the `hgs-tray.service` user unit. The service starts the selected
binary. The desktop entry runs `hgs-tray --sessions`,
which opens Sessions through the existing single-instance socket. A second
start forwards `show` or `sessions`; it does not create another GUI.

Fresh installs and upgrades default to Zerus Next. `HGS_LINUX_GUI=client`
selects it explicitly. Qt source builds and the Qt installer selection are
retired; existing Qt settings remain read-only migration input.

The client uses Qt's `com.hgdev.hgs-tray` identity and data root. Its first
start imports Qt and development-client drafts read-only and seeds its settings
once from Qt's INI store. Qt's own drafts and settings stay in place. The
updater snapshots both draft stores and retains native agents, tmux servers,
hgs services, and the mobile connector.

## Desktop background service

`hgs-desktop.service` is separate from the UI's `hgs-tray.service`. A normal
Next source installation registers the backend using the selected local CLI's
`hgs desktop registration --json` definition and readiness probe. Packaged
installations opt in as the desktop user, without sudo:

```sh
zerus-setup --desktop-service
```

The command writes this user's `hgs-desktop.service` under
`$XDG_CONFIG_HOME/systemd/user` (default `~/.config/systemd/user`) and enables
it for `graphical-session.target`. It imports the current login's runtime,
D-Bus and Wayland environment before starting the service. A genuine X11
session imports its display and clears stale Wayland data. The helper never
imports GUI-specific library paths into the user manager. Custom units,
drop-ins, symlinks and registrations for another configuration are kept for
manual review. Pacman never writes user homes or starts services.
Because the live Next source installer preflights this registration, a user
drop-in on `hgs-desktop.service` also blocks `packaging/desktop/install.sh` GUI updates until
the customization is deliberately reviewed and removed.

For a separate configuration, pass absolute `--config-dir` and `--state-dir`
paths and optionally `--hgs /absolute/path/to/hgs`. `--check` validates the
registration without changes. `--no-start` registers the next graphical login
without starting a child. Without a current graphical environment, registration
also defers startup; it never borrows stale display data from the manager.

A desktop environment must import its login environment before bringing up
`graphical-session.target`. Bare window managers need their own graphical
session integration. After that window manager has exported its current login
variables, run:

```sh
zerus-setup --desktop-service session
```

For a source installation, use
`python3 ~/.local/lib/zerus/install-linux-desktop-service.py session --hgs ~/.local/bin/hgs`.
Run this again when replacing the graphical session: importing environment into
the manager alone cannot update an already running child. The helper restarts
only the owned desktop backend when its session or backend changes. A UI-only
update keeps the unchanged backend alive. `install.sh` refreshes an already
managed registration after a deliberate CLI upgrade; a CLI-only installation
with no registration creates no desktop service. An inactive service stays
stopped, and an active backend upgrade without a current graphical environment
remains pending until an explicit session refresh. Package upgrades require the
same explicit `zerus-setup --desktop-service` step; pacman performs no refresh.
For SSH or agent-shell CLI upgrades without a graphical session, the helper
retains `pending_restart` and returns status 3. `install.sh` reports that notice
after completing its connector and worker updates and exits successfully; it
does not restart the desktop backend with stale graphical data. Other refresh
errors still fail the installer after those independent steps have completed.
The Next source GUI installer handles status 3 the same way and continues its
independent, draft-preserving GUI update. The backend still needs a refresh from
the current graphical session; other registration errors keep the GUI update
blocked.

Readiness must match the selected CLI's lifecycle ABI and configuration, with
`lifecycle.ipc_ready=true` and the systemd main PID. This establishes IPC
readiness only. Fresh registration runs passive `desktop serve`; an explicit,
reversible background selection is described below. Each background component
reports its separate status. Inspect `hgs desktop status --json` and
`journalctl --user -u hgs-desktop.service` when startup fails. No native agent,
tmux server, recovery, voice or mobile service is stopped by this helper.

### Select the desktop service mode

Use Zerus Next's maintained service-mode startup and authenticated **Grant**
against the passive service first. After a retained revoke, use its explicit
**Rearm** instead; changing the unit never clears a revoke. The GUI and service
must select the same configuration. The installer cannot grant ownership.

From a foreground terminal **outside Zerus**, in the current graphical login:

```sh
# Source checkout; packaged installations ship the same helper in lib/zerus.
python3 scripts/install-linux-desktop-service.py install --hgs "$(command -v hgs)" --mode background --json
python3 scripts/install-linux-desktop-service.py status --hgs "$(command -v hgs)" --json
```

The helper requires the actual current native ownership grant and checks the
original caller's process ancestry. Agent callers, including terminals launched
inside Zerus that carry `HGS_SESSION`/`HGS_RUN_ID`, are refused. The other supported
caller is the installed frontend's native Settings action. Missing or changed
grant, custom units/drop-ins, incomplete graphical environment and unknown
caller identity refuse without enabling background mode. No GUI is restarted.

Selection changes only the owned `hgs-desktop.service`, adding `--background`
and restarting that service through `systemctl --user`. `status` reports
`selected_mode`, `running_mode`, `pending_restart` and any pending transition;
`hgs desktop status --json` reports `lifecycle.mode` independently of component
readiness. A selected background mode does not prove every worker is ready.
Normal installer/session refreshes and CLI upgrades preserve the selected mode;
omitting `--mode` never changes it. `--no-start` and headless staging cannot
perform an explicit mode transition.

To return to passive service registration:

```sh
python3 scripts/install-linux-desktop-service.py install --hgs "$(command -v hgs)" --mode passive --json
```

This first checks the selected CLI's `desktop-revoke-hold-v1` help marker,
records one original rollback revoke UUID, and requires its exact durable,
checkpointed revoke (including runtime/resource drain). It then retains the
original native hold child through unit replacement, the desktop-only restart
and passive readiness. Every manager step checks that child before and after.
Only stdin EOF releases the exclusion; the matching release frame, actual child
exit and both output pipes must settle before the transition completes. See
[the native hold contract](desktop-revoke-hold.md).

A lost or unsettled result retains the original transition; inspect `status`
and repeat **the same passive transition**, which retains its UUID. An
unfinished rollback blocks background selection and ordinary refresh. If a new
authenticated Grant/Rearm won before the hold, the old UUID correctly refuses.
The same record can remain if the helper stopped after saving the transition
but before the original revoke took effect; the existing Service grant can
still be current. In either case, the owner can explicitly cancel only that
pending registration record:

```sh
python3 scripts/install-linux-desktop-service.py cancel-rollback --hgs "$(command -v hgs)" --request-id ORIGINAL_UUID --json
```

Cancellation requires the same native caller check and a verified current
Service grant. It changes no unit, selected mode, native receipt, revoke or
manager state, and records `last_cancelled_rollback_request_id`. A later
`install --mode passive` is a new explicit owner intent with a new UUID; no
unknown operation is automatically replayed. Never delete records or the
database to recover. Returning to background requires a fresh authenticated
frontend Rearm. The [Mac helper](macos-distribution.md#explicit-desktop-service-mode)
provides the corresponding LaunchAgent route. Windows mode switching remains
separate work; all fresh registration defaults are passive.

## Qualify in a scratch prefix

Use a clean checkout of the exact candidate and a new prefix. Never use the
owner's installed GUI or real `~/.local` for these checks.

```sh
export ZERUS_DEV_BUILD_NUMBER=NUMBER_FROM_THIS_MAIN_PUSH
export HGS_INSTALL_PREFIX="$PWD/target/linux-switch-check"
HGS_LINUX_GUI=client bash packaging/desktop/install.sh
```

A nonempty `HGS_INSTALL_PREFIX` has these boundaries:

- the binary is `<prefix>/bin/hgs-tray`;
- the launcher is `<prefix>/share/applications/hgs-tray.desktop`;
- the unregistered unit is `<prefix>/lib/systemd/user/hgs-tray.service`;
- rollback, provenance, update snapshots, and client drafts are under
  `<prefix>/share/hgs`, `<prefix>/state/hgs`, and `<prefix>/share/com.hgdev.hgs-tray`;
- no service-manager action occurs and no GUI starts.

Verify the installed identity and launch files:

```sh
"${HGS_INSTALL_PREFIX}/bin/hgs-tray" --build-info
grep -E 'ExecStart|Exec=' \
  "${HGS_INSTALL_PREFIX}/lib/systemd/user/hgs-tray.service" \
  "${HGS_INSTALL_PREFIX}/share/applications/hgs-tray.desktop"
python3 packaging/desktop/update-installed.py \
  --installed-gui "${HGS_INSTALL_PREFIX}/bin/hgs-tray"
```

The JSON must contain `CFBundleIdentifier: com.hgdev.hgs-tray` and
`ZerusGUI: tauri`; the last command prints `tauri`. The service must name the
scratch binary without `--sessions`; the desktop entry must include it.
`--build-info` may print renderer diagnostics before its JSON line.

macOS enables the client's `video` feature whenever it exists. Linux enables it
only when `pkg-config --exists gstreamer-1.0 gstreamer-app-1.0` succeeds;
otherwise the source installer keeps `media` and prints that round video is not included.
AUR builds always enable `video` and require the GStreamer build/runtime packages.
Runtime video also needs the encoders and muxers listed in
`client/media/README.md`.

Run the updater and installer suites and the media checks from the repository
root:

```sh
python3 -m unittest discover -s tests -p 'test_tray_update*.py' -v
cargo +1.99.1 clippy --locked --manifest-path client/media/Cargo.toml \
  --all-targets -- -D warnings
cargo +1.99.1 test --locked --manifest-path client/media/Cargo.toml --features video
bash -n packaging/desktop/install.sh
```

The matrix covers Qt to client, client to newer client, rollback to Qt, both
draft stores, a missing GStreamer development package, the default Next upgrade and explicit Qt
fallback, the numbered Dev guard, and the absence of real service calls.

## Switch the owner to Zerus Next

Work from a clean, persistent checkout of the exact main commit. Use the CI
sequence from that exact main push, not a semantic version or an unrelated run.
Check the running GUI's `--build-info` and
`~/.local/share/hgs/build-provenance.json`, coordinate the time, and ask the
owner to stop typing while Qt quits. Do not run `install.sh` as part of this
UI-only switch. The installer also registers the selected CLI's desktop
backend; an unchanged running backend stays alive.

```sh
cd CLEAN_MAIN_CHECKOUT
export ZERUS_DEV_BUILD_NUMBER=NUMBER_FROM_THIS_MAIN_PUSH
HGS_LINUX_GUI=client bash packaging/desktop/install.sh 2>&1 | tee ~/zerus-linux-switch.log
```

The updater quits Qt through its normal draft-saving path, swaps the binary,
starts the existing unit, waits for the single-instance socket, and sends
`sessions`. A failed start restores the previous GUI. After the new client is
verified, Qt remains in `~/.local/share/hgs/desktop-previous/hgs-tray`.

Verify without using the display:

```sh
~/.local/bin/hgs-tray --build-info
systemctl --user status hgs-tray.service
cat ~/.local/share/hgs/build-provenance.json
ls -l ~/.local/share/hgs/desktop-previous/hgs-tray
python3 packaging/desktop/update-installed.py --installed-gui ~/.local/bin/hgs-tray
python3 - <<'PY'
import json
from pathlib import Path
root = Path.home() / '.local/share/com.hgdev.hgs-tray'
print(json.loads((root / 'drafts/.imported-v1.json').read_text()))
print(json.loads((root / 'settings.json').read_text()).get('seed'))
PY
```

The build must have the selected Dev number and exact clean source commit. The
draft import marker must be complete, with no `unfinished` result. The settings
seed must name Qt's INI source and the keys actually carried over.

The coordinator and owner then check on the real display:

- hidden login start, the desktop entry, and a second `--sessions` invocation
  produce one instance and one tray item;
- the tray icon, tooltip, menu (Open Zerus, one line per computer, Quit, with
  no session rows) and attention state, including `Update available: Zerus N`
  and `Restart to finish update` when applicable;
- attention banners open their exact session and withdraw when it clears;
- Qt and development-client drafts and the seeded settings are present;
- native agents, tmux, hgs services, and the mobile connector are unchanged;
- sleep, wake, and ordinary use leave no main-thread watchdog report in the
  user journal.

## Previous Next build and retained Qt image

Use `python3 packaging/desktop/update-installed.py --rollback ~/.local/bin/hgs-tray` to
restore a verified previous Next build through the same draft/process checks.
Each successful update keeps its verified replaced Next build in
`desktop-previous`. A displaced Qt or unclassified rollback image gets a unique
entry under `~/.local/share/hgs/desktop-qt-legacy/qt-*/`; multiple older versions
can coexist and are never deleted or selected by `--rollback`. A failed legacy
move keeps the original in a named location and does not skip draft/process or
provenance checks for the verified Next update. Qt source builds and the
`HGS_LINUX_GUI=qt` selection are retired. Retained images are manual recovery,
not permission to start their background duties alongside the service.
