# A Windows computer as an SSH peer (WhiteWh/zerus)

Upstream Zerus manages Linux and macOS computers over SSH, and a Windows PC only
as the computer it runs on. The fork lets the Zerus desktop on one Windows PC
manage another Windows PC as a peer as well: its sessions appear in the session
list, New session offers it, and Activity, Terminal, send and answer work as for
any peer.

Phone access does not change: each Windows PC keeps its own mobile connector and
computer credential ([fork-mobile-tailscale.md](fork-mobile-tailscale.md)), and
the phone reaches every PC through the relay.

## How it works

- `hgs machine check <alias>` runs the POSIX probe first. When a Windows
  computer's `cmd.exe` cannot run it, a `cmd.exe` probe (`%OS%`, the
  architecture, `%USERPROFILE%\.local\bin\hgs.exe --version`) answers instead,
  and hgs remembers the alias as Windows in `~/.config/hgs/machine-platforms.json`
  (beside `machines.local.json`). `machine setup` is for Linux and macOS only.
- Remote commands for a Windows peer run `"%USERPROFILE%\.local\bin\hgs.exe"`
  with each argument quoted for its command-line rules. Windows OpenSSH hands the
  whole line to `cmd /c` with its own quotes. An argument holding `%` or `"`
  cannot pass through `cmd.exe` unchanged and is refused.
- The desktop takes each machine's platform from its own metrics, so a Windows
  peer's folders are Windows folders.
- **The session host is never started from an SSH session.** One started there
  would carry the SSH session's token (elevated for an administrator, which then
  shuts the peer's own desktop out) and would end with it. On the peer, turn on
  **Settings -> Sessions -> Keep the session host running for other computers**:
  its Zerus then runs `hgs session-host start`, a host that stays without
  sessions until `hgs session-host stop`, and checks it every minute.

## Setting up a peer

On the **peer** (the PC to be managed):

1. Install the fork's Zerus, start it, and enable local sessions (Machines).
2. Turn on **Settings -> Sessions -> Keep the session host running for other
   computers**.
3. Make sure both PCs can reach each other, for example in one Tailscale tailnet.
4. Enable OpenSSH Server for key logins from the managing PC. In PowerShell as
   Administrator (the account must be an administrator):

   ```powershell
   Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
   Set-Service -Name sshd -StartupType Automatic; Start-Service sshd
   $key = 'ssh-ed25519 AAAA... the managing PC''s public key'
   foreach ($file in "$env:ProgramData\ssh\administrators_authorized_keys", "$env:USERPROFILE\.ssh\authorized_keys") {
       New-Item -ItemType Directory -Force -Path (Split-Path $file) | Out-Null
       Add-Content -LiteralPath $file -Value $key -Encoding ascii
   }
   icacls "$env:ProgramData\ssh\administrators_authorized_keys" /inheritance:r /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F'
   icacls "$env:USERPROFILE\.ssh\authorized_keys" /inheritance:r /grant "${env:USERNAME}:F" /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F'
   Restart-Service sshd
   ```

   Grant by SID, not by name: on a localized Windows the group is not called
   `Administrators`, `icacls` then changes nothing, and sshd ignores a key file
   that others can read. The key goes to both files because `Match Group
   administrators` in the default `sshd_config` may not match a localized group.
5. Restrict SSH to the private network, for example the tailnet only:

   ```powershell
   Disable-NetFirewallRule -Name OpenSSH-Server-In-TCP
   New-NetFirewallRule -DisplayName "OpenSSH from tailnet" -Direction Inbound -Protocol TCP -LocalPort 22 -RemoteAddress 100.64.0.0/10 -Action Allow
   ```
6. If both PCs have the same computer name, give the peer its own hgs name:
   `%USERPROFILE%\.config\hgs\config` with `HGS_SELF="<name>"`.

On the **managing PC**:

1. Add the peer's host key to `~/.ssh/known_hosts` (connect once, or
   `ssh-keyscan -t ed25519 <host> >> ~/.ssh/known_hosts` over a network you
   trust, such as the tailnet).
2. In Machines, add the peer with its host name (for example
   `<pc>.<tailnet>.ts.net`) and the peer's **Windows user name**, then choose
   Check. The check reports `Windows_NT` and the peer's hgs version.

## Limits

- Project folders of a Windows peer stay on that computer: the shared catalog
  takes POSIX folders only, so New session on another computer browses the
  peer's folders instead of offering them by project.
- The peer's sessions start only while its session host runs (step 2 above).
- `hgs @peer session-host` stays refused: a computer manages its own host.
