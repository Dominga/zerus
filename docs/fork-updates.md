# Fork builds and updates (WhiteWh/zerus)

This fork adds a Windows portable build with self-update. None of this is part of
upstream Zerus; the branch `whitewh/main` carries it on top of upstream `main`.

## Builds

`.github/workflows/fork-windows.yml` runs on every push to `whitewh/main` and on
manual dispatch, only in `WhiteWh/zerus`. On a standard `windows-2025` runner it:

1. runs the Rust tests and builds the release `hgs.exe`;
2. builds the desktop app with the pinned Qt 6.10.3 and runs its offscreen Qt suites;
3. packages and self-tests the portable zip (`scripts/package-windows.py`,
   `scripts/check-windows-portable.py`);
4. publishes release `whitewh-<run number>` with `zerus-windows-x86_64.zip` and
   `zerus-fork-feed.json`, marked as the latest release.

A failing step publishes nothing. The run number is embedded in `hgs-tray.exe` as
`ZERUS_FORK_BUILD`, together with the feed address `ZERUS_FORK_FEED_URL`; CMake
accepts only release assets of `WhiteWh/zerus` for the latter.

An experimental build of any commit: push a tag `whitewh-exp-<name>` for it. The
same workflow builds, tests and packages that commit and publishes the zip as a
prerelease named after the tag. It is never the latest release and has no feed,
and its `hgs-tray.exe` embeds neither a fork build number nor a feed, so the
updater neither offers it nor replaces it. It is installed by hand, and going
back to the regular builds is a hand install of a `whitewh-<build>` zip.

## Feed

`https://github.com/WhiteWh/zerus/releases/latest/download/zerus-fork-feed.json`,
schema 1:

```json
{"schema": 1, "build": 7, "commit": "<40 hex>", "size": 123456789,
 "url": "https://github.com/WhiteWh/zerus/releases/download/whitewh-7/zerus-windows-x86_64.zip",
 "sha256": "<64 hex>"}
```

## Updates

`tray/src/ForkUpdater.cpp` is active only in a portable installation (a folder
with `build-info.json` next to `hgs-tray.exe`) of a build that embeds the feed.
Two minutes after start, then hourly, it reads the feed. When `build` is newer than
its own, it downloads the zip into `<installation>.update`, beside the
installation. Only HTTPS release URLs of this fork are accepted, up to 1 GiB. The
zip must match the feed's size and SHA-256, and its `build-info.json` must name
the feed's commit. Windows `tar.exe` unpacks it. The feed is read hourly even
while a build waits for the restart: a newer build downloads into
`<stage>-next` and replaces the waiting one only once it is verified, so the
restart always installs the newest build found.
**Settings -> Updates -> Check now** reads the feed at once.

A verified build shows **Restart to finish update** in the tray menu and the
sidebar, and posts one notification. Restarting quits Zerus as Quit does, so
drafts are saved. A PowerShell helper without a console window then shows a small
banner in the lower right corner of the screen with each step and its progress
(a click hides it) and:

1. waits until no program runs from the installation folder, naming the ones it
   waits for;
2. moves the folder to `<installation>.previous` (the single rollback copy);
3. moves the new build in and starts it again.

The banner leaves once the new build's window is up. A failure stays on it for
ten seconds and goes to the log.

If the new build exits within 15 seconds, the helper restores the previous build,
keeps the failed one as `<installation>.failed` and starts the previous build. If a
program still runs from the folder after two minutes, the update waits for the
next restart. The helper logs to `zerus-fork-update.log` beside the installation.

Native agents and the session host run from `%USERPROFILE%\.local\bin\hgs.exe` and
the staged host image, never from the portable folder, so updates do not stop
them. On its first start, the new build refreshes the staged CLI through
`session-host install` as any newer portable build does.

The first fork build is installed by hand. Builds made elsewhere, including
upstream releases, have no fork build number and never update this way.

### Versioned root

The plain layout above puts `.update`, `.previous`, `.failed` and the log next to
the installation, that is, in whatever folder holds it. A versioned root keeps all
of it inside one folder and never moves the running build:

```
C:\Tools\Zerus\
  current\      junction to the build that starts; shortcuts point here
  app-24\       the previous build, kept for rollback
  app-25\       the current build
  .update\      a newer build while it downloads and waits for the restart
  update.log
```

The updater uses it when `<root>\current` is a junction to the folder Zerus runs
from, started either as `current\hgs-tray.exe` or from that build's own folder.
After **Restart to finish update** the helper waits for Zerus to quit, moves the
verified build to `<root>\app-<build>`, re-points `current` at it and starts it.
Nothing has to stop running from the old build, so the two-minute wait of the
plain layout never applies. A build that exits within 15 seconds hands `current`
back to the previous one. After a successful start the two newest `app-*` builds
stay and older ones are removed.

When `current` points to another build than the one running, that build does not
update at all: start Zerus from `current`.

Switching an existing installation, with Zerus closed (`app` is its folder):

```powershell
cd C:\Tools\Zerus
New-Item -ItemType Junction -Path current -Target app
```

Then start `C:\Tools\Zerus\current\hgs-tray.exe` and point shortcuts, including the
Startup one, there. The first update goes to `app-<build>`; the original `app`
folder is not an `app-*` build and stays until it is removed by hand.

## Log

Zerus writes its warnings, every status-bar notice and each change of the
connection line to `~/.local/state/hgs/zerus.log`, beside the relay's
`relay.log`. **Open log** in the tray menu and **Log** in the status bar open it
in the system's text viewer. Past 4 MiB the file moves to `zerus.log.1`,
replacing the older copy.

## Optional extensions

Integrations with optional third-party tools stay outside the core as
extensions. Each has an id that names both its cargo feature (`src/extensions.rs`)
and its Settings → Extensions switch (`tray/src/Extensions.h`, QSettings key
`extensions/<id>`). Both are off by default: a plain `cargo build` keeps the
command but answers that the extension is absent, and a GUI built without
`ZERUS_FORK_BUILD` (or `-DZERUS_EXTENSIONS_ON_BY_DEFAULT=ON`) shows the switch off.
The fork workflow builds `hgs` with `--features socraticode`, and a fork GUI
turns extensions on by default.

- `socraticode`: `hgs socraticode …` and the SocratiCode strip in Details, with
  its native-Qdrant option.

## Windows mobile gateway additions

The step-by-step setup with Tailscale, from install to pairing a phone and the
usual problems, is in [fork-mobile-tailscale.md](fork-mobile-tailscale.md).

Upstream's Windows gateway (`docs/superpowers/specs/2026-10-20-windows-mobile-gateway-design.md`)
manages the bundled connector through `zerus-mobile-connector --manage`. The fork
adds, under Settings -> Mobile connection (`tray/src/WindowsMobileServices.h`):

- **Start the connector when Zerus starts** (`mobile/runConnector`, on by
  default): at startup Zerus asks `--manage status` and, for a configured
  connector that is not running, `--manage start` with that revision. It never
  stops or kills the connector.
- **Run a relay on this computer** (`mobile/localRelay`, off by default): the
  bundled `zerus-relay.exe` serves `127.0.0.1:8787` with its SQLite database in
  `%USERPROFILE%\.local\state\hgs\relay`, from a per-build copy under
  `%USERPROFILE%\.local\share\hgs\mobile\<hash>` so the portable folder stays
  replaceable. A phone needs HTTPS: publish it inside the tailnet with
  `tailscale serve --bg --https=443 http://127.0.0.1:8787` (Tailscale's HTTPS
  certificates must be enabled; Serve stays tailnet-only, unlike Funnel).
  Provision the computer once with the relay stopped:
  `zerus-relay --database <db> provision --name <workspace> --computer-name <name>`,
  then save the printed computer credential in Mobile connection.

## Windows SSH peers

The desktop on one Windows PC can manage another Windows PC over SSH; see
[fork-windows-peers.md](fork-windows-peers.md).
