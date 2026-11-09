WhiteWh fork
- This is a build of the WhiteWh/zerus fork, not of upstream Zerus. Fork build
  @FORK_BUILD@; it checks @FORK_FEED@ two minutes after start and hourly,
  and installs newer fork builds itself (Settings > Updates, Fork updates).
  Upstream releases never replace it. docs/fork-updates.md in the source has
  the details.
- hgs-tray.exe is the Tauri desktop. A versioned installation keeps it in
  <root>\app-<build> with <root>\current pointing at the running build; start
  Zerus from current\hgs-tray.exe.
- The fork ships only the Tauri desktop; the earlier Qt fallback
  (qt\zerus-qt.exe) is gone. Quit an old fallback before starting this build.
