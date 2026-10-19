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
the feed's commit. Windows `tar.exe` unpacks it.

A verified build shows **Restart to finish update** in the tray menu and the
sidebar, and posts one notification. Restarting quits Zerus as Quit does, so
drafts are saved. A hidden PowerShell helper then:

1. waits until no program runs from the installation folder;
2. moves the folder to `<installation>.previous` (the single rollback copy);
3. moves the new build in and starts it.

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
