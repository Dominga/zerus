Zerus @VERSION@ for Windows x86_64 (portable)
hgs @CLI_VERSION@, Tauri desktop, source commit @SOURCE_COMMIT@

Requirements
- Windows 10 version 1903 or later, or Windows 11, on x86_64.
- Microsoft Edge WebView2 Evergreen Runtime. Windows 11 normally includes it;
  on machines without it, install the Evergreen Runtime from Microsoft's page:
  https://developer.microsoft.com/microsoft-edge/webview2/#download-section
  Microsoft Edge itself is not a substitute. The runtime is shared with other
  applications and Zerus never removes it on uninstall.
- The Windows OpenSSH client (ssh.exe) for remote computers, with key-based
  SSH access and hgs installed there. Existing SSH aliases keep working.

Use
- Extract the whole folder and run hgs-tray.exe. This is the Tauri Zerus Next
  desktop; it ships no Qt runtime. Keep the files together: the desktop uses
  the adjacent hgs.exe and zerus-mobile-connector.exe.
- Closing the window keeps Zerus in the notification area. Run hgs-tray.exe
  again to bring it back. For portable login start, put a shortcut in
  shell:startup with --background appended to its Target. The per-user
  installer also offers an optional Start at login choice, off by default.
- The CLI works from a terminal as .\hgs.exe '@myserver' ls in PowerShell.
  With this folder on PATH, use hgs.exe '@myserver' ls. In Git Bash set
  MSYS_NO_PATHCONV=1 before passing remote paths such as /workspace/project.
- Configuration remains in %USERPROFILE%\.config\hgs (HOME when set).
  HGS_CONFIG_DIR and HGS_STATE_DIR keep their existing meanings. tray.conf
  is read from .config\hgs under HOME or the account's profile folder.
- Local sessions remain opt-in: use Machines or hgs.exe session-host install.
  The staged CLI and host images live outside this app folder, so replacing
  or uninstalling this folder does not remove native agents or their sessions.
- Desktop service setup remains separate from optional GUI login start.
  hgs.exe desktop registration --json previews the maintained logon definition
  and readiness contract. The installer has a marked hook for the Windows G1
  setup/removal command; it creates no ad-hoc scheduled task while that command
  is pending.
- The bundled Rust outbound connector keeps its existing configuration and
  durable request receipts. Closing Zerus does not stop native sessions.

Updates
The portable zip is distributed at https://github.com/ufna/zerus/releases.
The per-user installer is currently built and tested in CI; publishing that
installer is a separate release task. The installer preserves configuration,
state, drafts and staged native hosts, and removes obsolete app files only
inside its verified owned installation folder. For a portable update, quit
hgs-tray from its tray menu before replacing the app folder. Keep your data.
Compare the zip with the release's SHA256SUMS, for example:
  Get-FileHash zerus-<version>-windows-x86_64.zip -Algorithm SHA256
Unsigned builds can trigger Windows SmartScreen on first start.
Documentation: https://github.com/ufna/zerus/blob/main/docs/installation.md#windows

Licenses
Zerus is MIT-licensed (LICENSE). THIRD_PARTY_NOTICES.md describes project
components. This distribution includes:
- Microsoft Visual C++ app-local runtime DLLs.
- Microsoft.Windows.Console.ConPTY @CONPTY_VERSION@, under the MIT license
  (licenses\conpty-LICENSE.txt), with OpenConsole's third-party notices
  (licenses\conpty-NOTICE.md). hgs.exe stages the pinned pair for local hosts.
- Rust dependency notices for hgs and the connector (licenses\rust), and the
  Tauri desktop's selected Rust dependencies (licenses\rust\desktop).
- Locked frontend runtime dependency notices, including Vite's emitted preload
  helper (licenses\frontend).
WebView2 Evergreen is installed separately under Microsoft's terms.
