# Linux source installation and the Tauri client switch

This runbook covers a source installation on the owner's Arch Linux machine.
It does not change the AUR packages or their contents. The coordinated switch
is performed by design-ac with the owner; implementers qualify only scratch
installations and private headless sessions.

## What the installer preserves

`tray/install.sh` keeps the executable name `hgs-tray`, the application desktop
entry, and the `hgs-tray.service` user unit. The service starts the selected
binary without opening a window. The desktop entry runs `hgs-tray --sessions`,
which opens Sessions through the existing single-instance socket. A second
start forwards `show` or `sessions`; it does not create another GUI.

A fresh Linux installation defaults to Qt. Select the Tauri client explicitly
with `HGS_LINUX_GUI=client`. Later runs without that variable retain whichever
GUI is installed. `HGS_LINUX_GUI=qt` selects Qt again.

The client uses Qt's `com.hgdev.hgs-tray` identity and data root. Its first
start imports Qt and development-client drafts read-only and seeds its settings
once from Qt's INI store. Qt's own drafts and settings stay in place. The
updater snapshots both draft stores and retains native agents, tmux servers,
hgs services, and the mobile connector.

## Qualify in a scratch prefix

Use a clean checkout of the exact candidate and a new prefix. Never use the
owner's installed GUI or real `~/.local` for these checks.

```sh
export ZERUS_DEV_BUILD_NUMBER=NUMBER_FROM_THIS_MAIN_PUSH
export HGS_INSTALL_PREFIX="$PWD/target/linux-switch-check"
HGS_LINUX_GUI=client bash tray/install.sh
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
python3 tray/update-installed.py \
  --installed-gui "${HGS_INSTALL_PREFIX}/bin/hgs-tray"
```

The JSON must contain `CFBundleIdentifier: com.hgdev.hgs-tray` and
`ZerusGUI: tauri`; the last command prints `tauri`. The service must name the
scratch binary without `--sessions`; the desktop entry must include it.
`--build-info` may print renderer diagnostics before its JSON line.

macOS enables the client's `video` feature whenever it exists. Linux enables it
only when `pkg-config --exists gstreamer-1.0 gstreamer-app-1.0` succeeds;
otherwise the build keeps `media` and prints that round video is not included.
Runtime video also needs the encoders and muxers listed in
`client/media/README.md`.

Run the updater and installer suites and the media checks from the repository
root:

```sh
python3 -m unittest discover -s tests -p 'test_tray_update*.py' -v
cargo +1.99.1 clippy --locked --manifest-path client/media/Cargo.toml \
  --all-targets -- -D warnings
cargo +1.99.1 test --locked --manifest-path client/media/Cargo.toml --features video
bash -n tray/install.sh
```

The matrix covers Qt to client, client to newer client, rollback to Qt, both
draft stores, a missing GStreamer development package, installed-GUI
persistence, the numbered Dev guard, and the absence of real service calls.

## Switch the owner to Zerus Next

Work from a clean, persistent checkout of the exact main commit. Use the CI
sequence from that exact main push, not a semantic version or an unrelated run.
Check the running GUI's `--build-info` and
`~/.local/share/hgs/build-provenance.json`, coordinate the time, and ask the
owner to stop typing while Qt quits. Do not run `install.sh` or change any
service other than the GUI's existing unit.

```sh
cd CLEAN_MAIN_CHECKOUT
export ZERUS_DEV_BUILD_NUMBER=NUMBER_FROM_THIS_MAIN_PUSH
HGS_LINUX_GUI=client bash tray/install.sh 2>&1 | tee ~/zerus-linux-switch.log
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
python3 tray/update-installed.py --installed-gui ~/.local/bin/hgs-tray
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

Design-ac and the owner then check on the real display:

- hidden login start, the desktop entry, and a second `--sessions` invocation
  produce one instance and one tray item;
- the tray icon, tooltip, session menu, and attention state match Qt, including
  `Update available: Zerus N` and `Restart to finish update` when applicable;
- attention banners open their exact session and withdraw when it clears;
- Qt and development-client drafts and the seeded settings are present;
- native agents, tmux, hgs services, and the mobile connector are unchanged;
- sleep, wake, and ordinary use leave no main-thread watchdog report in the
  user journal.

## Later Dev updates and rollback

Run the same installer from the new exact main checkout without
`HGS_LINUX_GUI`. The installed client remains selected, and the original Qt
stays the rollback copy until G6 removal; there is no second rollback slot. The numbered Dev
guard refuses an unnumbered replacement before anything changes.

Rollback to Qt is one command from the source checkout:

```sh
python3 tray/update-installed.py --rollback ~/.local/bin/hgs-tray
```

Before Qt starts, the updater probes the selected `hgs` for
`Desktop revoke ABI: desktop-revoke-v1` and obtains either a settled revocation
or the explicit no-handoff no-op. An older CLI retains the plain path only when
the state directory has no `desktop` entry; any existing entry, including a
link, keeps Qt stopped because a handoff cannot be ruled out. Failure, timeout,
a malformed or pending receipt, or a configuration mismatch keeps Qt stopped.
Recovery inspects the same request ID and reason; it never invents a new UUID
or stops another service.

The same guarded replacement and verification runs in reverse. The client then
becomes the rollback copy, so a second `--rollback` returns to it. Drafts typed
in the client remain in `~/.local/share/com.hgdev.hgs-tray/drafts`; Qt does not
read them. Use `HGS_LINUX_GUI=qt` on later installations to stay on Qt, and file
a Bead for the reason for the rollback.
