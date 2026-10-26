# Four machines and an Android controller

This guide assumes four owned Windows x86_64 computers, PowerShell 7 Core and,
where OpenCode is used, Ubuntu 24.04 in WSL. The Linux account `alice` in examples is synthetic; replace it with the actual account.
The owner has specified that three
computers share a subnet, but has not supplied all names/addresses or the fourth
computer's network. Use the roles below; do not treat example addresses as real.
A native Linux computer may use the Linux receiver/connector directly.

| Role | Physical location | Suggested alias | Services |
| --- | --- | --- | --- |
| A | LAN | pc-a-win / pc-a-wsl | Controller, optional always-on private relay |
| B | same LAN | pc-b-win / pc-b-wsl | Native/WSL agents and outbound connector |
| C | same LAN | pc-c-win / pc-c-wsl | Native/WSL agents and outbound connector |
| D | other network | pc-d-win / pc-d-wsl | Same, via private overlay network |
| Phone | Wi-Fi/mobile data | separate device identity | Android app and private-network client |

A physical PC and its WSL user are different execution environments. Two logical
rows are expected when both own sessions. Give every environment its own machine
alias, connector credential and receipt directory. Do not clone databases,
configuration identities, SSH private keys or Desktop Commander tunnel credentials.

## 1. Copy a verified candidate to each PC

Use one source-matched Windows/Linux candidate, not binaries from different
archives. Verify the candidate manifest SHA-256 through your trusted delivery
channel, then run `scripts/package_fork_candidate.py --verify C:/path/to/pair`
from the matching checkout. This checks integrity, not publisher authenticity.

Extract `windows/zerus-windows-x86_64.zip` to a fresh versioned directory, such as
`C:/Tools/Zerus/<fork-version>-<commit>/zerus-windows-x86_64`. Keep the old directory
for rollback. Run the extracted `hgs.exe version --json` and
`hgs-tray.exe --build-info`; source, fork version and clean-source flags must match
the pair manifest. Check `pwsh.exe` reports `PSEdition=Core` and major version >=7.
The CLI, desktop, connector and optional relay must come from that same checkout.

For a **new standard-profile installation** on each Windows target, from PS7:

```powershell
$bin = 'C:/Tools/Zerus/REVIEWED_BUILD/zerus-windows-x86_64'
& "$bin/hgs.exe" version --json
& "$bin/hgs.exe" session-host install
& "$bin/hgs.exe" ls --local --json
& "$bin/hgs-tray.exe" --hgs "$bin/hgs.exe" --sessions
```

`session-host install` stages the tested ConPTY pair and per-user CLI; it does not
start an agent. Windows SSH peers expect `%USERPROFILE%/.local/bin/hgs.exe` for
the SSH login user. Do not change Windows OpenSSH's default command shell to
PowerShell merely for Zerus: the existing peer quoting/probe expects cmd.exe,
while actual Windows agent terminals use the verified PowerShell 7 backend.

The `pc-*-win` names above are display/connection aliases. The adapter's `machine`
field must use the actual `host` value reported by the intended `hgs ls --local
--json` environment. Do not assign a second internal identity to an existing
profile just to match a display label.

The development PC's existing `zerus-local/START.cmd` uses a separate profile.
Keep that profile and use its launcher; do not replace it with the new-install
commands blindly. A desktop, Desktop Commander adapter and mobile connector must
all select the same intended HGS state. A different profile can correctly show
an empty list; copying another profile's database is not the fix.

## 2. WSL / OpenCode on each applicable PC

List the exact distro with `wsl.exe --list --verbose`, determine its Linux account,
and install the matching receiver in that user's versioned directory. Use the
source checkout's `install.sh` with an explicit versioned `HGS_INSTALL_PREFIX`;
set `HGS_MOBILE_CONNECTOR=1` only when building the Linux outbound connector too.
Disable unrelated optional service installation during candidate staging.

```sh
export RUSTUP_TOOLCHAIN=1.99.1
export HGS_INSTALL_PREFIX="$HOME/.local/share/zerus-fork/builds/REVIEWED_COMMIT"
export HGS_MOBILE_CONNECTOR=1
export HGS_RECOVERY_SERVICE=0 HGS_SWARM_SERVICE=0 HGS_VOICE_SERVICE=0
bash /path/to/matching/zerus/install.sh
"$HGS_INSTALL_PREFIX/bin/hgs" version --json
```

The paired receiver can also be staged unchanged by the release procedure; compare
its SHA-256 with the pair. Keep the compatibility `hgs_state.py` beside it. Do not
substitute a Windows executable or a build-cache path.

Create a reviewed executable Linux wrapper for that installation and state:

```sh
#!/bin/sh
export HGS_SELF=pc-b-wsl
export HGS_CONFIG_DIR="$HOME/.config/hgs"
export HGS_STATE_DIR="$HOME/.local/state/hgs/sessions"
export HGS_OPENCODE_SERVICE_POLICY=existing-only
exec "$HOME/.local/share/zerus-fork/builds/REVIEWED_COMMIT/bin/hgs" "$@"
```

Use a different `pc-*-wsl` alias on every PC. No tokens belong in this script.
Install OpenCode separately under the same Linux user and start its **registered**
service with the owner's normal launch scripts. `opencode service status` must
report the intended version/address. An unrelated `opencode serve` process is
not the registered service. Zerus does not auto-start/replace it.

On each Windows desktop add its **local** WSL connection:

```powershell
$profile = @{transport='wsl-local';enabled=$true;wsl=@{
  distribution='Ubuntu-24.04';user='alice';
  hgs_path='/home/alice/.local/bin/zerus-hgs'
}} | ConvertTo-Json -Depth 5 -Compress
& $hgs machine set pc-b-wsl --json $profile --create
& $hgs machine check pc-b-wsl
& $hgs '@pc-b-wsl' account ls
```

This is not a route from A to B's WSL. A named `wsl-local` connection always runs
on the current Windows host. Use a direct outbound mobile connector inside B's
WSL for phone access; for desktop-to-remote-WSL access use an explicitly configured
SSH endpoint for that Linux user, or operate the local desktop on B. Do not label
a local distro as another computer to make routing appear to work.

## 3. Private networking and optional desktop SSH peers

Recommended: join A/B/C/D and the phone to one private Tailscale tailnet (or an
existing reviewed WireGuard/VPN design). All four can use the overlay, even when
A/B/C are in the same LAN. No exit node or public router port-forward is needed
for the topology shown here. Restrict grants to the exact controllers/devices,
SSH targets and relay HTTPS endpoint; membership alone is not least privilege.

For a Windows target B/C/D, install **OpenSSH Server** using Windows Optional
Features or elevated PS7 `Add-WindowsCapability`. Install OpenSSH Client on A.
Enable `sshd` deliberately. The installer creates an inbound TCP22 rule: restrict
it to the actual trusted LAN controller address/subnet and/or approved private
network addresses. Do not leave a broad rule merely because installation did.
Never publish SSH, OpenCode4096, OpenChamber3000 or Unity MCP27182 to the Internet.

Use per-controller SSH keys with passphrases. Put only the public key in the
correct target user's authorized_keys (administrator accounts have special
Windows OpenSSH handling). Verify host-key fingerprints over an independent
channel; do not set StrictHostKeyChecking=no. Choose a normal development user,
not an administrator, unless a separate operation really requires elevation.

An example local connection on A after key/host verification:

```powershell
$sshProfile = @{hostname='pc-b.YOUR_TAILNET.ts.net';user='YOUR_WINDOWS_USER';
  port=22;identity_file="$HOME/.ssh/zerus_pc_a";enabled=$true
} | ConvertTo-Json -Compress
& $hgs machine set pc-b-win --json $sshProfile --create
& $hgs machine check pc-b-win
& $hgs '@pc-b-win' ls --local --json
```

Repeat for C and D with exact users/addresses. `machine check` records Windows
versus POSIX peer behavior. A failed key/host check must be fixed, not replaced
with an implicit password or default-host fallback. From automated Windows tools,
use HGS's SSH routing rather than piping raw ssh.exe into synchronous shells.

## 4. One private HTTPS relay for the phone

Choose an always-on A (or an existing private server) and keep its database local,
private and backed up. The Windows delivery must include `zerus-relay.exe` to use
Settings -> Mobile connection -> Run a relay on this computer. Alternatively run
the matching native relay explicitly on `127.0.0.1:8787`.

Before starting, provision one workspace and separate credentials for **every
logical environment**. The administrative CLI requires the relay stopped:

```powershell
$relay = "$bin/zerus-relay.exe"
$db = "$HOME/.local/state/hgs/relay/relay.sqlite3"
& $relay --database $db provision --name 'My four computers' --computer-name 'pc-a-win'
# Record workspace_id and the private node_token securely, not in chats or Git.
& $relay --database $db node --workspace WORKSPACE_UUID --name 'pc-b-win'
& $relay --database $db node --workspace WORKSPACE_UUID --name 'pc-c-win'
& $relay --database $db node --workspace WORKSPACE_UUID --name 'pc-d-win'
# Issue additional pc-*-wsl credentials only for environments that own WSL sessions.
& $relay --database $db serve --host 127.0.0.1 --port 8787
```

Run only one relay instance against that SQLite database. The generated output
contains secrets: use a private trusted local terminal and protected storage;
do not paste it into ChatGPT/Desktop Commander responses or a shared log.

Expose the loopback relay **only inside the tailnet**:

```powershell
& 'C:/Program Files/Tailscale/tailscale.exe' serve --bg --https=443 http://127.0.0.1:8787
& 'C:/Program Files/Tailscale/tailscale.exe' serve status
```

Use Serve, not Funnel. Enable the required tailnet HTTPS/MagicDNS settings through
the owner's account; certificate transparency can disclose the chosen DNS name.
Use a neutral hostname. Do not bypass certificate validation on Android. A custom
reverse proxy with a trusted HTTPS certificate is an alternative; see the relay's
existing deployment guide for poll/body/proxy-header limits.

The v1 relay is a **trusted component**: HTTPS/private networking is not a claim
of end-to-end encryption between phone and agents. Use only an owner-controlled
relay. Encrypted v2 workflows have their own enrollment/acceptance requirements
and are not silently enabled by this guide.

## 5. Outbound connector on each environment

Windows: in that desktop's Settings -> Mobile connection, enter the private
`https://pc-a.YOUR_TAILNET.ts.net` URL and **that computer's** node credential,
Save, inspect the read-only configuration check, then Start connector explicitly.
Use the Settings save operation to create the owner-private Windows configuration
and management state; do not hand-copy a JSON file into an arbitrary shared folder.
The connector should use the same HGS/profile as the desktop and workstream
adapter. Do not reuse one node token for four computers.

WSL/OpenCode: run the Linux connector directly from its user session/service with
`hgs_path` pointing to the exact wrapper from step2. Private mode0600 config:

```json
{
  "server_url": "https://pc-a.YOUR_TAILNET.ts.net",
  "node_token": "PRIVATE_CREDENTIAL_FOR_THIS_WSL_ENVIRONMENT",
  "hgs_path": "/home/alice/.local/bin/zerus-hgs",
  "poll_interval": 5,
  "state_dir": "/home/alice/.local/state/hgs/mobile-pc-b-wsl"
}
```

```sh
chmod 600 "$HOME/.config/hgs/mobile/connector.json"
/path/to/versioned/zerus-mobile-connector --check --config "$HOME/.config/hgs/mobile/connector.json"
/path/to/versioned/zerus-mobile-connector --config "$HOME/.config/hgs/mobile/connector.json"
```

The connector opens outbound HTTPS; no inbound agent port is required. A Windows
connector's one-hop route is not substituted for native WSL OpenCode. The private
URL must resolve and validate from **inside WSL**, not just Windows. Fix DNS or
use the approved overlay client/network integration; never fall back to public
HTTP. Use a new receipt directory after changing relay/node identity, retaining
old uncertain-delivery receipts for review. Configure a user service only after
the foreground connection works and the explicit runtime paths are final.

## 6. Android companion

Install the supplied **Zerus Fork development APK** side-by-side with upstream
Zerus; its package is `app.zerus.mobile.romanilyin`. Verify its SHA-256 first. Allow
installation from the specific trusted file source, not indiscriminately. This
is not a signed stable Play distribution; preserve the signing key for upgrades.
Its public-upstream update channel is disabled and its default relay is empty.

Connect the phone to the same private network. Enter the exact HTTPS relay URL.
While the relay is stopped, create a fresh ten-minute, one-use phone invitation:
`zerus-relay --database PATH invite --workspace WORKSPACE_UUID`; restart the relay,
then pair in the app. Do not send the invitation through public channels. Phone
and computer credentials are distinct. Verify the four expected logical rows,
not just the fact that pairing succeeded.

Native Codex/Claude/Kimi on Windows retain their supported mobile actions.
OpenCode in WSL is controlled through its direct Linux connector. External Chat/
Work/Codex Chat rows display checkpoints and allow **cooperative queued text**,
not cloud prompts, terminal/stop or auto-approval. See external-workstreams.md.

## 7. Desktop Commander on four computers

Install the owner's DesktopCommanderMCP release/setup separately on each machine
and create a separate private tunnel/plugin connection. Keep the active runtime
versioned; do not replace it while a coding chat depends on it. Add the optional
`local/zerus-workstream.mjs` adapter from the matching source checkout and a private
per-machine profile pinned to the clean HGS source. It can use existing
`start_process` calls; no server restart or new privileged MCP tool is necessary.

Explicitly choose the correct machine tool and workstream in each chat. Register
Chat/Work/Codex Chat as declared by the user, then checkpoint/poll/ack at safe work
boundaries. Multiple chats still share the same OS account permissions; the
ledger is coordination, not filesystem isolation. Use separate worktrees when
concurrent changes would otherwise collide.

## Acceptance and rollback

On each computer check PS7, binary source identity, machine check, local/WSL
listing, model catalog and a harmless explicit file read. On the phone check
snapshot, history, one idempotent queued external instruction and its explicit
agent acknowledgement; never test by stopping an unrelated native session.
Check the fourth PC over mobile data/private network, not only the LAN.

A successful local build is not evidence that all four owner machines or the
physical phone have been installed. Record per-machine identity, environment,
source, connector state and test result without tokens. Keep previous Windows/
Linux components and the corresponding configuration together. Rollback selects
that pair; it must not restart native agents, re-key saved conversations or
replace a live desktop without its normal guarded update workflow.

Sources: Microsoft Windows OpenSSH first-use documentation; Tailscale Serve and
HTTPS certificate documentation; this source's mobile-deployment.md,
windows-wsl.md, fork-candidates.md and external-workstreams.md. Recheck vendor
commands when installing a newer platform version.

## Vendor references checked for this guide

- Tailscale Serve: https://tailscale.com/docs/features/tailscale-serve
- Serve command flags: https://tailscale.com/docs/reference/tailscale-cli/serve
- Windows OpenSSH setup: https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_install_firstuse
- Windows SSH key ownership: https://learn.microsoft.com/en-us/windows-server/administration/openssh/openssh_keymanagement
