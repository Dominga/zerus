# Phone access to a Windows PC over Tailscale (WhiteWh/zerus)

This guide connects the Zerus Android app to a Windows PC that runs the fork's
portable build, with no server of your own: the PC runs the mobile connector and
a relay for itself, and Tailscale carries the phone's HTTPS traffic to that relay
inside your private tailnet. Nothing is published to the internet.

```
phone (Zerus app) --HTTPS inside the tailnet--> tailscale serve on the PC
    --> zerus-relay.exe on 127.0.0.1:8787 <-- zerus-mobile-connector.exe --> hgs.exe
```

The Windows gateway is upstream's (see
`docs/superpowers/specs/2026-10-20-windows-mobile-gateway-design.md`): from the
phone you control the PC's Codex, Claude and Kimi sessions. The fork adds new
sessions from the phone, but only in a project's working folder on this PC: the
phone cannot browse the PC's folders or create new ones. A project's folders are
the ones Zerus shows under Projects for this computer (`hgs project ls`); the
phone sees them as `/D:/Work/project`. Worktrees, DeepSeek, OpenCode and voice
are not available from the phone on a Windows gateway yet.

## What you need

- The fork's Windows build `whitewh-31` or newer
  (<https://github.com/WhiteWh/zerus/releases/latest>). The zip carries
  `zerus-mobile-connector.exe` and `zerus-relay.exe` beside `hgs-tray.exe`.
- A Tailscale account (the free personal plan is enough).
- An Android phone with the Zerus app 0.2.0 or newer:
  <https://zerus.dev/downloads/android-v0.2.0/zerus.apk>. Uninstall an older
  `-dev` build first; it is signed differently.

## 1. Tailscale on the PC and the phone

1. Install Tailscale on the PC, for example `winget install --id Tailscale.Tailscale -e --source winget`,
   and sign in.
2. Install Tailscale on the phone (Google Play `com.tailscale.ipn`, or the APK from
   <https://pkgs.tailscale.com/stable/#android>) and sign in with **the same account**.
3. Both devices now appear in the admin console:
   <https://login.tailscale.com/admin/machines>.

## 2. HTTPS certificates for the tailnet

The Android app talks to its gateway only over HTTPS. Tailscale issues a real
certificate for the PC's tailnet name (`<pc>.<tailnet>.ts.net`):

1. Open <https://login.tailscale.com/admin/dns>.
2. Keep **MagicDNS** enabled.
3. Under **HTTPS Certificates**, choose **Enable HTTPS** and confirm.

Certificates come from Let's Encrypt, so the PC's tailnet name appears in the
public Certificate Transparency logs. That publishes the name only: the address
behind it (`100.x.y.z`) is reachable from devices signed in to your tailnet and
nowhere else. Rename the machine in the admin console first if its name says too
much.

Check from the PC (in its own folder, it writes the certificate files there):

```powershell
& "C:\Program Files\Tailscale\tailscale.exe" cert <pc>.<tailnet>.ts.net
```

"your Tailscale account does not support getting TLS certs" means step 2 is not
done yet.

## 3. The relay on the PC

1. Start Zerus, open **Settings -> Mobile connection** and turn on
   **Run a relay on this computer (127.0.0.1:8787)**. Zerus starts the bundled
   relay now and at every start; its database is
   `%USERPROFILE%\.local\state\hgs\relay\relay.sqlite3`.
2. Publish it inside the tailnet:

   ```powershell
   & "C:\Program Files\Tailscale\tailscale.exe" serve --bg --https=443 http://127.0.0.1:8787
   ```

   `tailscale serve status` should show `https://<pc>.<tailnet>.ts.net (tailnet only)`.
   Use `serve`, never `funnel`: Funnel would publish the relay on the internet.
3. Check it: <https://<pc>.<tailnet>.ts.net/healthz> answers
   `{"ok":true,"protocol_version":1}` from the PC, and later from the phone.

## 4. A computer credential for this PC

The relay issues credentials only through its command line, and only while it is
stopped (it locks its database). Turn **Run a relay on this computer** off,
then in PowerShell:

```powershell
cd $env:USERPROFILE
$relay = "D:\Software\zerus-windows-x86_64\zerus-relay.exe"   # your Zerus folder
$db = "$env:USERPROFILE\.local\state\hgs\relay\relay.sqlite3"
& $relay --database $db provision --name "My workspace" --computer-name $env:COMPUTERNAME
```

It prints the workspace and computer identities, a phone invitation and the
`node_token`, this computer's credential. Keep it private: do not paste it into
chats, issues or files you share. Turn the relay on again.

To replace a lost or leaked credential later, issue another one for the same
workspace (`... node --workspace <workspace_id> --name <name>`) and revoke the old
computer (`... revoke-computer --id <computer_id>`), both with the relay stopped.

## 5. The connector

In **Settings -> Mobile connection**:

1. **Relay (HTTPS):** `https://<pc>.<tailnet>.ts.net`.
2. **Replacement computer credential:** the `node_token` from step 4.
3. **Save configuration**, then **Start connector**.
4. Keep **Start the connector when Zerus starts** on (the fork's default) so you do
   not have to start it after each login.

The status turns to *Connector: running*. The connector runs `hgs.exe` from the
Zerus folder; closing Zerus leaves the connector, the relay and every agent
running.

## 6. Pair the phone

1. On the PC: **Machines -> Phones -> Pair a phone...** shows a QR code and a
   code, valid for 10 minutes and once.
2. On the phone, with Tailscale connected: Zerus -> **Machines -> Pair another
   workspace**, then scan the QR code, or enter the gateway URL
   `https://<pc>.<tailnet>.ts.net` and the code, and choose **Pair**.
3. The PC appears under Machines as *Gateway, Online* and its sessions under
   Sessions.

## Troubleshooting

**"Read timed out" on the phone, `/healthz` never loads in the phone's browser,
but works on the PC.** The direct UDP path between the devices drops packets
larger than its MTU (common on mobile networks and some routers). From the PC:

```powershell
& "C:\Program Files\Tailscale\tailscale.exe" ping --size 1250 <phone>
& "C:\Program Files\Tailscale\tailscale.exe" ping --size 1300 <phone>
```

If 1250 answers and 1300 does not, make Tailscale use its DERP relays (TCP, no
packet-size limit) for this PC. In PowerShell **as Administrator**:

```powershell
New-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tailscale' -Name Environment -PropertyType MultiString -Value 'TS_DEBUG_ALWAYS_USE_DERP=true' -Force; Restart-Service Tailscale
```

`tailscale status` then shows the phone as `relay "<region>"`. To undo it:
`Remove-ItemProperty -Path 'HKLM:\SYSTEM\CurrentControlSet\Services\Tailscale' -Name Environment; Restart-Service Tailscale`.
Do **not** set `TS_DEBUG_MTU` below 1280 on Windows: IPv6 requires at least
1280, the tunnel cannot start and Tailscale stays at "no backend".

**The phone shows the PC twice, with duplicate sessions.** The relay has two
computer credentials for it, an old one and the current one. Stop the relay and
revoke the stale computer (Offline, older "last seen"):
`... revoke-computer --id <computer_id>`. The phone's refresh drops it.

**The connector refuses its state directory.** Upstream's connector creates its
private files with owner-only ACLs and refuses existing data it did not create
instead of repairing it. Save the configuration again from Mobile connection with
a new credential: it starts a fresh receipt directory beside `connector.json`.

**Names do not resolve on the phone.** Turn off Android's Private DNS, or set it
to Automatic, and keep "Use Tailscale DNS" on in the Tailscale app. Security apps
that filter web traffic can also block `*.ts.net`.
