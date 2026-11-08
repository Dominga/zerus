WhiteWh fork
- This is a build of the WhiteWh/zerus fork, not of upstream Zerus. Fork build
  @FORK_BUILD@; it checks @FORK_FEED@ two minutes after start and hourly,
  and installs newer fork builds itself (Settings > Updates, Fork updates).
  Upstream releases never replace it. docs/fork-updates.md in the source has
  the details.
- hgs-tray.exe is the Tauri desktop. A versioned installation keeps it in
  <root>\app-<build> with <root>\current pointing at the running build; start
  Zerus from current\hgs-tray.exe.

Qt fallback
- @QT_FALLBACK@
- It is not the default and it does not update itself. Start it by hand only
  if the Tauri desktop does not work for you: quit Zerus (the tray menu's
  Quit), then run qt\zerus-qt.exe from this folder. It uses the hgs.exe of
  this folder and the same accounts, sessions and agents.
- The Tauri desktop imported the Qt settings and composer drafts once, on its
  first start; changes you make in the fallback do not reach it.
- Updates still come only through the Tauri desktop. The next fork releases
  drop the fallback.
