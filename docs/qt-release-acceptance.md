# Zerus Next release acceptance and Qt removal

## G3 scope for 0.48.0 — owner decision 2026-11-05

Qt migration is limited to **projects/organization and accounts**. The existing
one-time preference import is already delivered and stays in place; accounts
already live in the shared `hgs` configuration. Everything else from Qt is
**configure again**: drafts, unsent organization edits, attention/notification
history and identities, pending creations/launches, task-message/task-Start
recovery, unknown deliveries, repository clone settings and caches. Original
Qt files and attachments stay untouched for manual recovery. Qt operations
are not imported, executed, replayed or offered as Next recovery choices.
Previously imported Next copies are not cleared.

This supersedes the earlier full-family G3 source-work requirements and import
row descriptions below; dated run records remain historical evidence. No new
capture families are required. One plain GUI explanation is kept until **Got
it** is saved in Next's local settings, then does not appear on later starts.
A save failure keeps it available. Replays and dev profiles do not acknowledge
the production profile's notice.

Next's **own** interim stores are not Qt migration. Drafts, creation/clone
checkpoints, read queues, task-message outboxes, task-Start records and original
service-handoff markers keep their existing roots and guards. Unresolved Starts
stay review-only; reserved messages keep their original IDs/recovery-only path.
Missing/mismatched pairs refuse a fresh attempt. Notice acknowledgment changes
no pending marker and grants no effect or retry permission.

For G3 upgrade acceptance, verify projects/accounts and existing Next choices,
untouched Qt sources, no Qt draft recovery or old-operation/notification replay,
Next pending markers across restart and the once-only notice. Native caller,
platform and deletion gates remain independent; no platform box is checked.


## 0.48.0 acceptance sessions — owner decision 2026-11-05

Qt is removed in 0.48.0. Agents execute the unattended acceptance rows on each
owner computer; the owner supplies one short session per computer (about
20–30 minutes) for notification click and microphone/camera consent. macOS additionally retains its `.66.3.40.4` owner row.
The projects/accounts-only G3 scope above and the clean-install exception below
apply. This decision supersedes the older scheduling restrictions in dated
records; it does not promote source tests or readiness into installed acceptance.

On Arch, linux-shell-u owns the unattended rows and the evidence/runbook task
`zerus-5wmh.66.8.1.22`. Every unattended display operation requires a fresh
60-second input-idle observation, the original shared Arch display flock held
through the operation, an unlocked session and the same verified GUI/build.
Abort on owner input, lock, stale observation or changed target. No native agent,
tmux server or DeepSeek host may be restarted. Updates use the maintained
installer/updater and are announced by the coordinator.

Desktop-u exclusively executes `.66.3.42` **inside the Arch owner session**:
one scoped Post after normal GUI Quit, followed by the owner's **one physical
Open-session click**. Linux-shell-u hands over that slot and does not duplicate
its Post, click or launch. No automated click worker runs in the owner slot.
Owner input is expected in this explicit session; it does not waive idle
admission for unattended operations.


Owner: `zerus-5wmh.66.8.1` (G6). Basis: the
[retirement inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md),
especially its responsibility tables, migration contract and G1–G7 gates.
The [Qt parity checklist](qt-parity-checklist.md) records which user-facing Qt
capabilities Zerus Next has, with a bead for each gap.
**Historical scheduling (2026-11-02; superseded by the 2026-11-05 execution
decision above):** installed acceptance uses the owner's
**main accounts and existing canonical install route**, with no separate test
account. **macOS runs first once the source admission gates below are merged**;
Mac GUI/native checks need no separate owner window. **Arch GUI acceptance is
deferred until the owner reschedules it.** Windows remains on va-kids using the
owner's main account; the inventory's unreachable-PC note is historical.
**Full platform acceptance remains pending.** Recorded subchecks do not establish
platform acceptance; a merged PR or a private fixture pass does not check a box.
Owner removal decision (2026-11-03): **#906a retires Linux/macOS Qt builds and
CI only after both macOS and Linux acceptance records are complete.** It is a
ready review PR, not permission to merge or deploy. **#906b remains draft until
Windows PC (va-kids) acceptance** and then removes the retained `tray/` tree and
Windows Qt pieces. Windows PC testing does not gate #906a. Use the existing
candidate installation, never either deletion branch, for acceptance.
**Owner decision (2026-11-05): every acceptance row without recorded evidence is
waived** (Mac notifications/clicks `.66.3.30.2.4`, voice/video, menu bar/lifecycle,
terminal and service registration/readiness, and UI/durable parity on all three
platforms); the platform tables mark them "WAIVED — owner 2026-11-05".
**Owner decision (2026-11-05): Qt rollback is waived on every platform** ("откат
на Qt не нужен совсем"). No Qt rollback or Qt round trip is prepared or run; the
rows below that relied on the round trip were re-planned by the coordinator.

For each platform, record the exact Next and last-Qt release/source, installed
`--build-info`, package/artifact identity and installer route in the acceptance
PR comment. Fill every row with PASS or a linked blocking bead and the actual
steps/results. Keep logs bounded and redact account/message/device data. Preserve
Qt settings, drafts, outboxes and connector journals; never restart agents, tmux
or a live DeepSeek host, and do not touch unrelated services. Close every window
opened by a run and avoid notification spam. Explicit confirmations remain
required for destructive UI. **Clean-install exception, owner decision 2026-11-03:** use the exact candidate's
CI install/package checks plus the upgrade path on the owner's main accounts.
Do not reset an account, create a test account or require initially empty
accounts/projects. Record both evidence sets; the exception is not a native
clean-account PASS. Default-OFF subcases still need genuine default state: never
clear or reseed the owner's existing state to manufacture it; leave them pending
if they cannot be observed on the main account.

### Owner participation and the clean-install exception

The 2026-11-03 exception replaces all three clean-account subcases with exact
candidate CI install/package checks **and** the owner-account upgrade. Keep links
and results for both halves beside each platform row. It does not waive upgrade,
one-time import, service readiness, notifications or genuine default-OFF
checks. Qt rollback is separately waived by the 2026-11-05 owner decision.
Staging prefixes and fake service managers remain component evidence.

macOS automation may run without a separate owner window under the existing
rules. Ask the owner to perform any new microphone/camera, Apple Events, screen
lock/unlock or authentication consent; never approve privacy prompts for them.
Voice/video quality and any sensory result not observable in the retained
record require their observation. Arch uses the 2026-11-05 idle/display admission above for unattended rows;
the owner-only rows remain in its one scheduled owner session.
Windows PC execution waits for its separate acceptance. Neither this scheduling
nor the exception permits clearing state, creating a replacement account or
restarting agents, tmux or a live DeepSeek host.

### Qt rollback waiver — owner decision 2026-11-05

The Qt rollback/round-trip acceptance row is **waived by owner 2026-11-05** on
**every platform**. It is removed from owner sessions and remaining acceptance
work. Do not prepare or execute a Qt rollback for these rows. Earlier dated
results remain historical evidence; they do not create a new rollback or
return-to-Next requirement.

### One-time import coverage — coordinator decision 2026-11-05

Linux and macOS one-time import are **covered by fixture/CI evidence; no
owner-GUI swap**. Do not Quit the owner's canonical GUI to run a private-profile
import fixture. The per-euid production GUI exclusion is independent of HOME,
TMPDIR and XDG paths: a private start may forward to the owner GUI. A replay or
development identity is also not a substitute for the production notice.
This supersedes the earlier Arch/Mac private-profile fixture assignments.
No owner profile is seeded, cleared or reset.

The approved shared evidence for the Linux/macOS row is:

| Evidence | Scope and attribution |
|---|---|
| Windows private-profile `importonce` rerun | **PASS, producer-attributed shared fixture coverage:** both never-saved and unreadable-organization variants passed 18/18 on `861e0baff`, containing #1480/#1482. Linux Qt uses the same INI format/reader. Exact CI, artifact, preserved-source checks and limits are recorded below; Windows legacy per-computer lists were refused, not imported. |
| Real-shaped store and Organization regressions | The approved #1480/#1482 producer evidence covers INI and plist never-saved organization, malformed/unreadable inputs, backups and organization mentions, plus the actual Organization worker. The worker tests use a fake CLI and are labelled as such, rather than installed/native-GUI evidence. |
| Real CLI recovery (#1482, taskview-cl) | The producer ran the real CLI with Qt at its Linux probe path: never-saved organization imports the default; malformed input bootstraps with the pending marker while retaining the visible source refusal; a later readable fix imports the project and clears that marker. These are actual CLI observations, separate from the fake-CLI worker controls. |

This is the coordinator's accepted coverage route within the owner's release
direction for the import row, not permission
to replay Qt operations or import Qt drafts. Preserve projects/accounts scope,
original sources and existing Next data. The Windows rerun receipt below
completes the approved shared fixture evidence; it does not create a
Linux/macOS native-run claim or require another owner-GUI session. Other native
service, consent and platform checks retain their independent requirements.

### Received Windows shared import evidence — 2026-11-05

Win-accept-cl reports two unattended private-profile runs on va-kids, console
session 1, from integrate `861e0baff28d4f354eeaedcbcbc9387b27ecaaec`.
#1480 (`2f48ab984`) is an ancestor; #1482 is that source's merge commit.
[CI 42010395529, attempt 2](https://github.com/ufna/zerus/actions/runs/42010395529/attempts/2)
is green without a source change: package job `127208000281`, native job
`127207945053`. Attempt 1 remains failed on three WindowsTaskMode tests
(`zerus-yuwgz`), although its package job passed.

Artifact `13408459971`,
`windows-x86_64-861e0baff28d4f354eeaedcbcbc9387b27ecaaec` (portable 0.47.0), has
GitHub artifact SHA-256
`b7819cdb35197793db36877e698d9718266c60f56d9e181f22c8602534cba3bb`.
The run/source/artifact digest were independently checked; the actual private
fixture observations below remain **win-accept-cl's producer evidence**.

| Variant | Native observed result |
|---|---|
| Never-saved organization, stage `9edc0289` | **18/18 PASS.** The seeded INI contained no organization bytes. At starts 1 and 3, `swarm get` was initialized, New project was enabled, and Projects showed the local-catalog explanation without not-set-up/unavailable text. Legacy project lists were **Needs review**, with `legacy_projects_windows_unsupported`; the seeded alias did not appear in Projects. |
| Unreadable organization, stage `31cbad9e` | **18/18 PASS.** The seeded INI had `organization=not-a-saved-organization`. At starts 1 and 3, Projects remained initialized with New project enabled, while visibly reporting the unreadable saved organization and retry-at-next-start reason. Migration report showed Qt preferences **Waiting to retry** for `workspace/organization`; legacy project lists followed it. |

Both variants showed Upgrading from Qt only on start 1. Got it persisted
`qt_projects_only_notice_048`; starts 2 and 3 had no notice. Qt INI/draft bytes
and write/creation times were unchanged. The Qt draft was absent from the
composer and Next drafts; no Qt draft import or replay occurred. The legacy
`project ls` bytes and shared account profiles stayed unchanged; only the
runtime `account_status` cache filled. Producer teardown/guard/residue checks
were clean.

Limits remain explicit: these were small fixture seeds, **not real Qt profiles**
or real-account installations, and no Linux/macOS native GUI ran. Windows
legacy per-computer folders were refused and remain under coordinator review;
this report does not accept that missing Windows support. The unreadable case
stayed unreadable across its restarts: later readable repair was not exercised
on Windows. `gui_import_pending` was absent from the captured `swarm get`
output, so no value is inferred. Later repair/import/marker clearing is supplied
only by taskview-cl's separately attributed real-CLI #1482 evidence above.

### Retained legacy images after #1217

This guidance applies after [#1217](https://github.com/ufna/zerus/pull/1217)
lands; its source approval is not an installed acceptance result. Each displaced
Qt or unclassified rollback image can add a separate `qt-*` entry under
`~/.local/share/hgs/desktop-qt-legacy/`. On Linux, an updater staging prefix
uses `<HGS_INSTALL_PREFIX>/share/hgs/desktop-qt-legacy/`; the macOS updater ignores
that prefix and uses the normal root above. These entries are manual recovery
copies, never candidates offered by `--rollback`. Retained images have no
automatic expiry or size quota. On Linux, a replaced Next image whose
`--build-info` fails is conservatively retained too: the folder name does not
establish that its contents are Qt.

Review one entry at a time using the updater's printed path and the file
manager's size/properties view; inspect saved metadata without launching the
retained GUI. Keep recovery copies while their upgrade/import acceptance or any
uncertain update remains unresolved. A failed archive move can leave an image in
its named `desktop-previous/.replaced-*` location or the named archive entry;
inspect both before treating either as disposable.

Once an entry is verified as an obsolete recovery copy that no running GUI or
process uses, it may be removed manually. Keep it if ownership, image identity
or recovery needs remain uncertain. There is no bulk or age-based cleanup: an
unclassified image needs individual review. Preserve the installed image and the
usable `desktop-previous` rollback, Qt settings exports, original migration snapshots,
drafts and journals. Removing an image is not permission to clear owner state,
reseed an import or start a legacy GUI alongside service-owned background duties.

## Linux — arch

The 2026-11-05 owner assignment authorizes unattended installed checks under
the idle/display guard above, with the owner-only rows in one scheduled session.
Use the owner's main account and maintained canonical AUR/package or
`packaging/desktop/install.sh` route. Staging prefixes are file checks only, not installed
acceptance. Do not install a test binary by hand.

| Pending check | How to verify | Coverage / owner |
|---|---|---|
| [x] Clean-install exception — approved coverage | Exact-source `743ba9f41` Linux/Arch CI and package/install guards passed; the maintained owner-account upgrade is recorded separately below. No account reset or empty-account/native clean-install claim. | G5 `.66.7.1`, [#867](https://github.com/ufna/zerus/pull/867); linux-shell-u |
| [x] Upgrade with agents running | PASS — owner-authorized maintained Next-to-Next rollout, clean `7a60dceeb` to `8e74aa884`; exact session/job/native births, Terminal and unsent draft survived. See the dated Arch owner upgrade record below. | [#886](https://github.com/ufna/zerus/pull/886), G2 `.66.4`, G6 `.66.8` |
| [x] One-time import — approved shared coverage | Covered by the coordinator-approved Windows fixture/CI plus INI/plist/Core/CLI evidence above; no owner-GUI swap or Linux-native import claim. Projects/accounts only; preserve the explicit Windows legacy-list and later-repair limits. | G3 `.66.5.2.1`, `.66.5.3`, [#870](https://github.com/ufna/zerus/pull/870) |
| [x] Service registration/readiness, UI-closed presence/catalog | PASS for the recorded installed registration/readiness, custom-unit refusal and short owner-window closed-GUI presence/catalog cases below. A new UI-closed durable-read receipt remains an automated subcase. | `.66.3.27`, [#886](https://github.com/ufna/zerus/pull/886), [#898](https://github.com/ufna/zerus/pull/898), `.66.3.31` |
| [x] Notifications/clicks — exercised owner case | PASS (`.66.3.42`) on installed `8e74aa884`: after normal GUI Quit, one synthetic Post produced exactly one native alert; the owner's one physical Open-session click carried the matching Wayland activation token and opened one new GUI on the exact target and conversation. See the [Arch notification, media and unattended record](#recorded-arch-notification-media-and-unattended-results--2026-11-05). Stale/withdrawn refusal and lock/idle stay open automated subcases with test coverage, not owner gates (coordinator, 2026-11-05). | `.66.3.30.1/.3/.4`, [#940](https://github.com/ufna/zerus/pull/940) in-process service Linux adapter, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; [#894](https://github.com/ufna/zerus/pull/894) is extraction only; linux-shell-u/desktop-u |
| [x] Tray/lifecycle — exercised functional route | PASS for the exercised route on `8e74aa884`: an actual tray-menu Quit exited the GUI while the same background backend stayed serving; one maintained GUI-unit start produced one tray object; a second background invocation kept that GUI's PID/birth. See the [Arch unattended record](#recorded-arch-notification-media-and-unattended-results--2026-11-05). | G4 `.66.6`, `.66.3.31` |
| [x] Terminal — exercised owner cases | PASS: existing-session upgrade survival and the short-window space/non-ASCII folder, actual shell argv/cwd, prompt, paste and PTY resize 52×103 → 45×71. The cancelled test-shell reattach-after-client-exit case remains automated; no worktree was created. | inventory “Terminal and process lifetime”; G7 `.66.2`, native terminal contracts |
| [x] Voice/video — capture, preview and discard | Microphone: PASS for capture, stopped preview, audible playback and owner-confirmed discard. Camera: capture/control/discard PASS, with P2 playback-quality observation `zerus-6ioin`. Both on `8e74aa884`; see the [Arch media record](#recorded-arch-notification-media-and-unattended-results--2026-11-05). Transcription and recording recovery across exit stay open automated subcases (coordinator, 2026-11-05). | G2 `.66.4.5`, inventory “Voice, video and speech providers” |
| WAIVED — owner 2026-11-05 | UI/durable parity: waived by owner 2026-11-05 (no recorded evidence). | G1–G4/G7; [#884](https://github.com/ufna/zerus/pull/884), `.66.3.32`, `.66.3.35`, `.66.3.38` |
| WAIVED — owner 2026-11-05 | Qt rollback is waived by owner 2026-11-05 ("откат на Qt не нужен совсем"): no Qt rollback or Qt round trip is prepared or run on any platform. | `.66.3.31`, `packaging/desktop/update-installed.py`; desktop-u |

Linux notification acceptance also requires the root service attachment
(`.66.3.30.11.2`, desktop-u) and activation/click handoff to the service-mode UI
(`.66.3.30.12`, linux-shell-u). The merged adapter's component fixtures do not
complete these routes or the native checks with Next closed. Keep this row pending
until the coordinator's maintained owner route verifies readiness, presentation,
click routing, stale/withdrawn refusal and Wayland behavior.


### Arch 0.48 pre-owner preparation and nondisplay subchecks — 2026-11-05

Installed source: `7a60dceeb61423853766cd7072357c0a8e5b03c7`, clean,
`0.47.1-preview.design-polish.7a60dce`. The canonical GUI and CLI hashes match
installation provenance and their actual running images. The GUI caches
`ZERUS_CLIENT_DESKTOP_SERVICE=1` and maps the maintained repaired WebKit prefix;
no Qt/KF/WebEngine mappings were observed. These are installed identity
subchecks, not the clean-install exception's exact-candidate CI/package proof.

| Subcheck | Actual observation / remaining work |
|---|---|
| Registration and selected/running mode | Maintained installed helper `--check` passes; read-only status reports selected and running `background`, IPC ready, no pending restart or transition. One user-manager backend has no drop-ins. |
| Background readiness | Same backend PID/birth; lifecycle `serving`, background mode and actual attention host ready, delivery writer ready; automatic approvals remain OFF. No mode switch, Grant/Rearm or service restart was performed. Closed-GUI read delivery and host ACK still need their actual observations. |
| Custom registration refusal | Maintained helper `--check` against one private custom unit refuses before any `systemctl` execution. Both that unit and the real owner unit remain byte-identical. This is a private refusal subcheck, not an installed custom-unit mutation. |
| Display admission | A passive Wayland input observer and one retained display flock admitted bounded screenshot inspection. AT-SPI's advertised socket refused connection; no accessibility action occurred. Input resumed before a later Settings admission, so it refused before issuing a click. A subsequent admitted maintained navigation selected the exact live linux-shell-u target in the same original GUI; a separately admitted single click focused its verified empty composer. Draft/Terminal preparation, interactive parity and lifecycle cases remain pending. |
| Upgrade preparation | A private bounded manifest retains original native PID/births and draft hashes. The coordinator moved the next maintained rollout (after #1467 and #1470) into the first Arch owner-session segment because the owner is actively using Arch. Idle-window attempts for draft/Terminal preparation stopped, and the original observer joined. No draft text was typed and no **manifests ready** receipt was sent. Fresh manifests are taken in that explicit session; this preparation is not upgrade survival evidence. |
| Qt artifact | The retained previous GUI reports clean Qt source `3a28fb2219fa0c4e7a367933d35b4c1882f607d6`, Dev 1595. Before the waiver, build-info inspection opened no Qt window. No Qt rollback or mode change was run. Further preparation stopped; this acceptance row is waived by owner 2026-11-05. |

Private evidence is indexed by the Arch 0.48 acceptance task. Public records
omit account/message content and use the exact source plus selected-field
observations. None of these subchecks checks a complete platform row.

### Recorded Arch owner upgrade segment — 2026-11-05

**PASS, scoped installed Next-to-Next upgrade:** the owner-authorized baseline
was clean `7a60dceeb61423853766cd7072357c0a8e5b03c7`; the maintained
`arch-cli-preview.sh full` route (`install.sh`, `packaging/desktop/install.sh`, then
`update-installed.py`) returned 0 at 11:35 MSK and installed clean
`8e74aa88460d9b166542a98b57c8344606b1d5f8`,
`0.47.1-preview.design-polish.8e74aa8`. The CLI remains `hgs 1.56.0`.
The coordinator's updater result records 137 preserved drafts (6 unsent, 131
form-state-only, 0 held deliveries) and 57 native processes untouched.

The original native Codex, `hgs` wrapper, tmux server and bounded owned job keep
their exact PID/birth identities. The same native session/run/conversation,
tmux pane and terminal device remain; the live host still observes the original
job under that run/conversation. All 57 retained native agent/server image and
birth observations match. The one additional entry in the earlier census was a
tmux **client**, not a server, and is no longer present. No agent, tmux server or
DeepSeek host was restarted.

The synthetic draft was typed once and never sent. Its saved bytes are identical
after upgrade, including the exact text/attachment and unsent identity. The
owner physically selected Terminal and supplied screenshots before and after:
both show the exact linux-shell-u target **Connected**, its live Codex TUI and
unchanged tmux session. The post-upgrade screenshot does **not** show the
composer. The later 11:58 MSK microphone-preview screenshot does show that
exact synthetic draft still unsent, adding owner-provided visual corroboration
to the byte-level check. Automated tab/screenshot checks were incomplete; their
failure is not attributed to a product defect.

Exact-candidate package/CI evidence remains separate. At rollout time no run was
yet dispatched for `8e74aa884`; the coordinator reported green integrate run
41989037251 at `af07efb67` plus reviewed #1470/#1473/#1474 deltas. This is not
labelled exact-candidate CI/package proof. The subsequent closing
[run 42012026961](https://github.com/ufna/zerus/actions/runs/42012026961) selects
exact source `8e74aa884` through the workflow input (Linux and Arch enabled).
It **failed** in the Linux CLI native Codex clear fixture with a busy/approval
refusal (`zerus-suqrs`); no production cause is attributed. Its workflow-definition
head `743ba9f41` is distinct from the checked-out source. This failed run is
not relabelled green by the later exact-source run below.
Existing-session Terminal survival is
recorded; folder/argv/resize/paste and the other full acceptance rows retain
their separate checks.

### Recorded Arch notification, media and unattended results — 2026-11-05

These observations use clean installed source
`8e74aa88460d9b166542a98b57c8344606b1d5f8` and the maintained repaired
WebKitGTK/JavaScriptCore 2.54.2 prefix. The owner session began at 11:19 MSK;
the owner confirmed the final video discard and ended participation at
12:05:43 MSK. This exceeded the original 35-minute plan. No further owner input
is requested for this recorded run. Qt rollback was waived and never executed.

| Subcheck | Result and exact scope |
|---|---|
| Open session after GUI Quit (`.66.3.42`, desktop-u) | **PASS.** Normal Quit and original GUI exit preceded the sole synthetic Post. Exactly one native alert, ID 1460, received the owner's one physical Open-session click: matching Wayland activation token, `ActionInvoked/open-session`, then close reason 3. One new GUI using the same installed executable opened and selected the exact nonce-bearing target and conversation, confirmed by the owner screenshot. The synthetic label was restored; backend PID, epoch, generation and configuration were unchanged. No injected callback, automated click, second Post or retry occurred. Popup appearance, stale/withdrawn clicks and lock/unlock are separate unqualified subcases. |
| Microphone and playback | **PASS for capture, stopped preview, audible playback and owner-confirmed discard.** The three-second recording used the existing speech profile; the owner confirmed everything was audible. The first Discard screenshot still showed the confirmation dialog; the later explicit owner confirmation establishes the discard. The recording and original composer draft were not sent. Transcription and recording recovery across exit were not exercised. |
| Camera and playback | **Capture/control/discard PASS with P2 quality observation `zerus-6ioin`.** The owner recorded a five-second video, reported all controls working, but described playback as darker/lower quality. No controlled capture/playback comparison establishes its cause. This recording was in a **different session**, with an empty composer and model `claude-opus-5-5`; its exact session identity was not captured, so it is not attributed to the linux-shell-u upgrade target. The owner explicitly confirmed Discard at 12:05:43 MSK. No Send was requested; no data loss or privacy fault was observed. |
| Native tray lifecycle, unattended idle window | **PASS for the exercised functional route.** Under fresh idle60 and the original display flock, a fresh actual tray-menu Quit exited the retained GUI (pidfd confirmed). The same background backend stayed serving/IPC-ready while it was absent. One maintained GUI-unit start produced the same-source GUI and one active tray object, initially without a visible window. A second background invocation preserved that GUI's PID/birth; actual StatusNotifier primary activation showed its one window. All three nonempty Next composer drafts retained their keys, text and attachments. Panel artwork/menu appearance and lock/unlock were not freshly qualified by this route. |
| Service registration and current readiness | **PASS for installed registration/readiness and the private custom-definition refusal.** Installed helper status reports selected/running background, no transition or pending restart. Actual attention/native host and delivery writer are ready; approvals remain OFF. The custom unit refused before any manager call and both unit files stayed unchanged. No additional Grant/Rearm, mode switch or backend restart was performed. |
| Bounded catalog and exact host read | **PASS for the observed read-only data.** A bounded indexed local service page was complete, online and current (49 rows at that observation); no catalog contents are published. Exact-target `inspect --read-observation` returned complete/observed/present metadata and the matching current run/conversation and raw reply read mark after the owner activation. Its generation/time are retained privately. This is one host acknowledgement, not proof of a pending read delivered while the GUI was closed. |
| Longer closed-GUI presence/read probe | **NOT RUN.** The later bounded five-minute admission found no idle60; it performed zero Quit/start/catalog/presence steps. The earlier short closed-GUI lifecycle observation proves backend continuity, not a new presence heartbeat or durable read delivery during that absence. No input source is inferred from the observer, and no idle guard was bypassed. |

The actual activation-started GUI was initially outside the GUI unit
(`MainPID=0`); that did not authorize another GUI. Only after its confirmed
normal exit did the unattended lifecycle run start the maintained unit. At
closeout that managed GUI is running from the same installed object, the
original backend is still background/serving with attention and delivery ready,
and approvals remain OFF. The passive input observer and bounded owned job
joined; display flocks are released. No native agent, tmux server or DeepSeek
host was restarted or stopped. The discarded recordings were not submitted.
A read-only observation of the original GUI's process tree found no Qt/KF/
WebEngine mappings or directly held camera/audio device handles; it does not
claim coverage of another app or the whole PipeWire graph.

Private selected evidence is under
`.ci-build/arch-048-acceptance-20261105/` in linux-shell-u's checkout:
`upgrade-result.json`, `root-42-result.json`, `root-42-callback-proof.json`,
`media-voice-preview-observation.json`, `media-camera-observation.json`,
`unattended-tray-summary.json`, `target-host-read-observation.json` and
`closeout-observation.json`. `selected-evidence-index.json` binds these eight
selected records (each below 64 KiB) by size/hash. Desktop-u retains the full bounded DBus callback
capture and sender/process/image mapping under
`.ci-build/g6-arch-42-owner-arch42-n35fa2/`. Owner images and message contents
stay private. These independently scoped records are not a full platform PASS.

The exact-source CI/package and shared import receipts are now attached in
this record. Remaining automated cases stay with `zerus-5wmh.66.8.1.22`:
extended UI-closed read delivery; stale/
withdrawn notification and lock/idle cases; folder terminal with spaces and
non-ASCII, argv/resize/paste; transcription/exit recovery; and the remaining
inventory UI/durable walk. Actual current OFF is observed; clean-default origin,
authenticated opt-in and unknown-result handling are not inferred from it.
The owner segment is finished; these gaps do not request another owner action
or waive their guards. Keep the corresponding broad Linux rows pending until
the missing observations or a specific owner decision closes them.

### Recorded Arch short wrap-up window — 2026-11-05

The owner explicitly reopened an owner-present window and acknowledged hands
off at 13:47:03 MSK; idle60 was not required. This run binds the then-installed
clean `743ba9f41905ce2561e213f05529b2ae8de025b5`, rather than relabelling the
morning `8e74aa884` evidence. It includes the merged #1480/#1482 changes.

- **UI-closed presence/catalog PASS.** Normal GUI Quit and retained-pidfd exit
  preceded a 35-second absence. The same background backend PID/epoch remained
  serving/IPC-ready, published a fresh presence heartbeat and served a complete,
  current indexed catalog page containing the exact original target. One
  maintained GUI start restored the same build; three nonempty drafts stayed
  intact. This is not a new UI-closed durable-read receipt.
- **Terminal folder/prompt/paste/resize PASS.** The owner started one synthetic
  Run-shell session in a non-Git folder containing a space and `Ω`. The actual
  retained shell was `/usr/bin/bash -l` at that exact cwd, with no Git/worktree
  creation. The owner confirmed a prompt, ran `ls`, then pasted a harmless
  command; its exact output and cwd were checked. The native PTY changed from
  **52 rows × 103 columns to 45 × 71** after the owner's resize, confirmed by
  both `stty` files and owner screenshots.

At the owner's stop request, the proposed GUI Quit/reopen check was cancelled;
no test-shell reattach-after-exit result is claimed. Only the exact original
test shell received normal `exit`; its retained pidfd reported exit and its
original pane disappeared. Its four known files and folder were removed, as
were only its exact version-bound catalog folder/session links through the
maintained CLI. No other agent or tmux server was stopped. The display flock
is free. Window-size restoration was not confirmed after owner focus changed;
no further owner step or store edit was requested.

The coordinator closed further owner checks: stale/withdrawn click refusal,
transcription/recovery and the remaining UI walk stay documented as **open
automated subcases with test coverage, not additional owner gates**. Neither
those tests nor this stop decision create an installed PASS for an unrun case.
Private `owner-second-summary.json` and adjacent original-process, PTY,
closed-GUI and cleanup records retain the evidence; owner images stay private.

Exact-source Linux/Arch CI for this newly observed installed source is
[run 42014169811](https://github.com/ufna/zerus/actions/runs/42014169811), **green**.
[Run 42012026961](https://github.com/ufna/zerus/actions/runs/42012026961) separately
tests the morning `8e74aa884` source; it cannot qualify the newer install by
its workflow-definition head alone.

The successful resolver and Arch package job both checked out exact
`743ba9f41`; workflow definition `e2d461b75` remains a separate identity.
Arch package job `127200484398` built and installed
`zerus-git 0.47.0.r6457.g743ba9f41-1`. Its package layout/version/build-info,
Qt/KF dependency and Qt native-plugin absence guards passed, followed by the
unprivileged disposable-home setup and isolated terminal check. Arch desktop
`127200484492`, Linux CLI `127200484327` and Quick `127200484361` also passed.
Artifact `13410438906`, `arch-x86_64-743ba9f41905ce2561e213f05529b2ae8de025b5`,
has GitHub artifact SHA-256
`a80942adb8f697afd34d4075709da385e93d1f1c39e1a8acebcac817c34df471`.
This supplies the exact-source CI/package half of the approved clean-install
exception alongside the separately recorded maintained owner-account upgrade;
it is not a native clean-account installation or a blanket platform PASS.

### Arch owner session plan — original limit 35 minutes

Prepare first: build and stage the coordinator-selected candidate through the
maintained route, without updating yet. Finish nondisplay checks. Take the
fresh live manifests in segment 1, rather than waiting for an unattended idle
window. Settle prior unknown transitions, confirm
initial mode/Grant and dangerous opt-ins, and have desktop-u and its exact idle
synthetic target ready. Devices and the existing speech provider must be
available; no privacy or notification preference is silently changed.

1. **Upgrade with agents running (5–7 minutes).** In this explicit owner-present
   window, the owner keeps hands off for about 1–2 minutes while linux-shell-u
   holds the display flock, types its synthetic **unsent** draft and verifies
   the Terminal. Those expected test inputs need no idle60 admission. Capture
   the exact live session/run/conversation, jobs, process births and draft, then
   send **manifests ready**. The coordinator performs the maintained rollout and
   announces its exact SHA. Verify the same native agent/jobs/terminal and unsent
   draft survived without a restart before continuing. Do not begin the rollout
   from a preliminary manifest or an unconfirmed preparation.
2. **Notification / Open session (6–8 minutes; explicit handoff to desktop-u).**
   Desktop-u rechecks source, GUI/backend and target, holds the display flock,
   normally Quits the GUI and makes one fresh scoped Post. The owner physically
   clicks **Open session once**. Desktop-u records the native notification and
   callback identity, one relaunch and exact target selection, then restores
   that same installed GUI. A failed/unknown attempt is recorded without retry.
3. **Microphone/camera (6–8 minutes).** The owner answers the actual consent
   prompts and records a short synthetic phrase/clip. Check preview, Escape/
   cancel, transcription and draft retention, with the owner confirming sound
   and image quality. Do not send an uncertain recording again.
The planned segments total 17–23 minutes, within the original 35-minute cap.
Qt rollback is waived by owner 2026-11-05 and is not a session segment. The candidate is
built before the session; no compilation time is hidden in that estimate. Record
exact sources and evidence for each completed row. A blocked or unknown result
stops its dependent segment rather than triggering a replay or extending the
session. Keep the acceptance PR draft until session results are recorded.

### Recorded Arch passive cutover subchecks — 2026-11-02

The admitted main-account run used the existing canonical source installation
`a1023d4142d1efcffada146d9d1ad68f9596e6a6`, not the Qt-deletion draft.
webkit-c held the display lock for its 18:53–19:01:45 MSK client checks;
migrate-u performed the nondisplay checks. The coordinator verified restoration
and closed the window. **These are scoped subchecks; every Linux platform
checkbox above remains pending.**

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| Installed identity and dependencies | PASS, existing source install | GUI/CLI image hashes match the installed provenance for `a1023d4`; GUI reports Tauri and `com.hgdev.hgs-tray`. Direct ELF dependencies and the resolved `ldd` closure contain no Qt/KF/WebEngine or missing libraries. This was not a clean install or a package transaction. |
| Existing service registration and IPC | PASS, passive scope | The maintained installer `--check` succeeds. The owned user unit has no drop-ins; its selected image/configuration/PID and actual Wayland, runtime-directory and session-bus environment agree. The same service serves schema 15 with IPC ready. This does not establish background readiness. |
| Custom-unit refusal | PENDING | No owner unit or drop-in was changed to manufacture a negative case. |
| Service-mode startup, explicit Rearm and restart | PASS, passive scope | Startup respected the native/rearm fence. The explicit UI Resume obtained service ownership at revision 13; normal Quit retained it, and a fresh service-mode GUI obtained its original Grant at revision 14. The UI remained unavailable with zero Fleet source rows and no GUI Fleet fallback; no background runtime started. |
| Native restore | PASS, passive prior runtime | Normal native startup found its original accepted, durable `native_takeover` receipt with `native_ready` and revoked/checkpointed/drained fields. Ownership returned to native revision 15 with current identity. This is not evidence that an active notification or delivery runtime drained. |
| Existing drafts and sampled process continuity | PASS, sampled scope | webkit-c retained byte-identical contents for 37 nonempty draft files and unchanged original PID/birth identities for 24 sampled agent/tmux processes through Quit/restart/native restore. This is not an all-process census or a Qt-upgrade result. |
| Existing preference import | PASS, handled-family subcheck | Nine schema-1 preference families are handled, three with zero source keys; no family is quarantined. All nine recorded source hashes match the current Qt file. Its bytes, mtime and file identity, all nine family records and the original seed are unchanged after cutover/restart/restore; layout presentation metadata is present. No reseed or source write was performed. |
| Current approval policy and generic agent refusal | PASS, passive subcheck | The existing policy stayed OFF. A generic agent-shell OFF request was refused with `handoff_required`. This proves that passive refusal only, not active frontend authentication, ENABLE delivery or a clean default-OFF origin. |
| Active background readiness and UI-closed duties | BLOCKED: `.66.3.27.5` | The registered command is `hgs desktop serve` without `--background`; no maintained switch starts the active runtime. Ownership Grant/Rearm alone does not start it. Fleet, presence and read delivery therefore remain untested while the UI is closed. |
| Frozen transfer, raw host read and active approval delivery | BLOCKED: `.66.3.27.5` | No synthetic target or genuine mark was created. No transfer or exact-target raw host acknowledgement was inferred from the overlay, queue, IPC or ownership receipt. No approval ENABLE was submitted. |
| Service banner/click, active withdrawal and Qt rollback | BLOCKED: `.66.3.27.5` | No notification, active withdrawal/drain, Qt rollout or rollback was performed. |
| Optional-question preference and foreground suppression | KNOWN GAP: `.66.3.30.11.13` | The typed transport is not merged. These cases were excluded without failing unrelated passive subchecks. |
| Clean/default state and complete migration acceptance | PENDING | Existing owner state was never cleared or reseeded. Clean install, original default OFF, malformed-source recovery, last-Qt upgrade and rollback/forward idempotence remain unproved. |

The GUI returned through its maintained launcher with the same installed image
and native startup mode; the temporary mode flag was absent from both GUI and
manager. The desktop service retained its original PID and definition, remained
passive and IPC-ready, and kept approvals OFF. No install, service restart, unit
edit, notification, or agent/tmux/DeepSeek signal was performed. Opened test
windows were closed and webkit-c released the display lock. The coordinator then
resumed the rollout watcher; later replacements are outside this recorded run.

Selected nondisplay evidence is retained privately by migrate-u under
`.ci-build/migrate-u/arch-installed-acceptance-a1023d4/` (under 100 KiB at close).
webkit-c's bounded commands, receipts, draft/process comparisons and restoration
are in `.ci-build/installed-cutover-20261102/ledger.json` (8,110 bytes) and adjacent
selected records. Exact private identities stay there; this document contains no
message bodies or owner settings dump.

### Recorded second Arch active-service window — 2026-11-03

Tracked by `zerus-5wmh.66.8.3`. The owner rescheduled this window and performed
the maintained foreground mode switches. Physical checks used the existing
source installation `5e4427f6792e4d0b9315e470cf23f85e69a8aa65`, reporting Tauri,
`com.hgdev.hgs-tray` and `0.47.1-preview.design-polish.5e4427f`, with
`sourceDirty:false`. The earlier `926b046b35f96594e2581a31ebce000a2bda84ab`
preflight was read-only and superseded before effects; its observations do not
qualify the later installation. **This window remains incomplete; every platform
acceptance checkbox is unchanged.** It was not a package qualification, last-Qt
upgrade, Qt type rollback or Qt removal.

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| Installed identity | PASS, existing source install | Selected CLI/GUI hashes and running image identities matched the exact `5e4427f6` installation before effects; the restored Native GUI used the same installed image. No clean install or package transaction was tested. |
| Maintained background selection and readiness | PASS, active runtime scope | The owner's foreground maintained helper selected background successfully. Actual service status showed `background_service`, writers enabled, two live Fleet sources, presence `published`, reads ready, and the Linux attention host ready with its attachment restored and outbox pending zero. Automatic approvals stayed OFF. These are readiness observations, not a delivered read, banner or click. |
| Service GUI restart/reconnect | FAILED: `zerus-yqie` | Automatic reconnect failed after restart. The owner's manual Retry connected. This failed subcheck remains assigned to menu-c; a manual connection does not qualify automatic recovery or last-known catalog behavior. |
| Service session/project grouping | FAILED: `zerus-gpz3` | After Retry, the owner observed all 47 sessions in Inbox. Source diagnosis found service baseline/delta rows projected with an empty `row.machine`, while Native mode fills the local host or configured peer alias used by membership keys. The coordinator independently confirmed authoritative shared projects and memberships intact; no project deletion occurred. The fix and rerun belong to menu-c. |
| Retained Qt/Settings report | WARNING: `.66.5.11` | The retained Qt file's mode 0644 and the checkpoint's `0o077` refusal provide a sufficient source/metadata cause for the report warning; its detailed native error code was not directly observed. This independent best-effort Staged report neither gates the live connection nor imports or deletes live projects. No source permissions were changed to bypass it; complete G3 acceptance remains pending. |
| Owner Quit and maintained passive restoration | PASS, original helper transition | After owner-confirmed normal Quit, the original GUI invocation exited successfully. The same maintained helper completed passive restoration using its retained original `ARCH_G6_ROLLBACK_ID`, with no pending transition/restart. A post-restart lookup of that original revoke returned `writer_epoch_changed`; it was retained as a refusal, never upgraded into a successful receipt or replayed. |
| Fresh Native restoration | PASS, scoped restoration | Current status showed passive mode, no background runtime, writers disabled and no service owner epoch. The prior ABSENT startup flag was restored before one maintained Native GUI launch. A fresh authenticated `ARCH_G6_NATIVE_TAKEOVER_ID` was accepted and durable, with `native_ready`, revoked/checkpointed/drained, current Native revision 50 and current identity. This separate fresh takeover does not change the earlier revoke lookup's result. |
| Managed draft files and sampled processes | PASS, sampled continuity | All 174 managed regular nonempty Qt/Next draft-file hashes were unchanged, with none added, missing or changed. All 24 sampled original process birth identities and images were unchanged. This does not prove unsent-content semantics or an all-process census. The retained Qt source hash was unchanged from the regression probe. |
| Native project presentation after restore | PENDING, owner observation | The actual Native GUI was restored at the exact installed `5e4427f6` image with the service-mode environment absent. The owner's visual confirmation of project presentation was still pending when this ledger was prepared. |
| Remaining delivery/native/platform cases | INCOMPLETE | No deliberate Post/banner/click, synthetic read mark, exact-target host ACK or approval ENABLE was submitted. Terminal, voice/video, destructive-review refusal, complete migration, Qt rollback and package acceptance were not qualified by this window. G6 requires a rerun after the reviewed menu-c fixes and the remaining native cases. |

Root made no project/source edits or source-permission changes during the
physical window. Approvals remained OFF. Normal GUI Quit and the maintained
service transitions did not restart or signal the sampled agents, tmux or a
DeepSeek host. The shared display lock is released; the owner
visual observation above remains pending. These later active results do not
rewrite the scope or blockers of the historical 2026-11-02 passive run.

Evidence is retained privately under `.ci-build/g6-arch-5e4427f6/`; historical
read-only preflight records remain under `.ci-build/g6-arch-926b046b/`. Private
request UUIDs and process identities remain in those records; the stable labels
above identify their roles without publishing their values. Installed image
SHA-256 values were:

- CLI: `c2b2171f6e0f2049c3efda22e10583963b699f3d5abfafcd5b42497fe12cfe2d`.
- GUI: `b898362d3411f0f986d4a0bf63668b574eda35a37a17e52a306b8c21f73cee3e`.

The following SHA-256 values identify exact retained evidence bytes, not their
contents or an additional acceptance result:

| Private evidence filename | SHA-256 |
|---|---|
| `preflight-images.json` | `965879d4cee152e98eed02d9c8fad99a186f2bf8b017da13c65e87fffee6b288` |
| `status-after-background-switch.json` | `381dc2ad67b45234414cc514cdd32cefb4625ef29e0d1569e0792f1802819393` |
| `restore-helper-status.json` | `d227af53388e58563f3e6a34a459ed67ce8a6d50739f8fc512e6d457ede55bc8` |
| `restore-original-revoke-status.json` | `3987cd50bf5f58753a68a4daa060b8911802e5b18cce08cf32b7caa29fd4a1c7` |
| `native-restored-takeover-receipt.json` | `4f558744a11d0731cda4907b74a47c002b202292b49d3e2168dd534b6d497c0c` |
| `native-restore-continuity-summary.json` | `c351b54aad72fdcc547c6cddf858f6354bd0ff3ed7610fc79c9a3f596b533151` |

### Recorded Arch reconnect/grouping retry — 2026-11-03

This separate retry belongs to `zerus-5wmh.66.8.3` and product task
`T-i0p1w7`. Physical checks used the exact installed source
`5c11005546fc8bd0c6a2224acb81eacb5a21e0d9`, including
[#1124](https://github.com/ufna/zerus/pull/1124) and
[#1128](https://github.com/ufna/zerus/pull/1128). **The reconnect and project
presentation retry passed; broader G6 remains open and all platform checkboxes
are unchanged.**
The preceding `5e4427f6` failures and its pending historical observation are
unchanged; the results below apply to this new installed source only.

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| Installed identity and controlled GUI start | PASS, source-install scope | Selected CLI/GUI hashes matched installed provenance for `5c110055`, with clean source identity. The coordinator paused the rollout watcher; the original display lock was held. Owner-confirmed normal Quit ended the prior GUI with exit 0. The prior ABSENT startup flag was saved, only the service-mode flag was set to `1`, and one maintained service-mode GUI was started with its original process birth and unit invocation pinned. |
| Ownership after owner Resume | PASS, scoped Grant/current identity | After one owner Resume, the fresh lookup of `ARCH_G6_RETRY_GRANT_ID` found an accepted, durable `grant` result at revision 56. Its lookup retained `definitive:false`; the accompanying current snapshot independently showed Service revision 56, current identity and `rearm_required:false`. This Grant request is not relabeled as a Rearm request. |
| Maintained background switch and automatic reconnect | PASS, owner-confirmed retry | The owner's foreground Konsole call to the maintained helper succeeded for the same configuration: selected/running background mode, IPC ready, no pending transition/restart. After the service restart, the same original GUI automatically reconnected without Retry. The owner confirmed all 47 sessions and their projects in place. This qualifies the observed restart path for `zerus-yqie` and grouping path for `zerus-gpz3`, not every recovery case. |
| Background runtime and native frontend | PASS, readiness scope | Final status at service generation 66 and new service epoch `ARCH_G6_RETRY_SERVICE_EPOCH` showed writers enabled, local and Mac Fleet sources live, presence writer active, reads ready, automatic approvals OFF/disabled, and the Linux attention host ready with its attachment restored and outbox pending zero. The same original GUI's frontend observation remained `live_next` with its unchanged `ARCH_G6_RETRY_FRONTEND_INSTANCE`; the observer's `authority:false` did not grant mutation permission. This is readiness evidence, not notification or exact-target read-delivery acceptance. |
| Initial passive view and migration warning | OBSERVED / WARNING, G3 remains open | Before a live baseline, the fresh passive GUI initially showed a zero/skeleton view; this did not establish deletion. The retained migration-review notice associated with the Qt 0644 source remained visible. [#1133](https://github.com/ufna/zerus/pull/1133) was not installed in the attested `5c110055` run. The independent report warning does not overturn the later owner-confirmed project placement. |
| Duplicate new-session placeholder | SEPARATE BUG: `zerus-zckh` | A duplicate placeholder for a newly created session was reported separately to menu-c. Root did not create or exercise a new session to test it; this ledger does not qualify its fix. |
| End state requested by owner | PASS, background intentionally retained | The owner requested staying in background mode. The manager flag therefore deliberately remains `1`, changed from its prior ABSENT state; the registered service remains background. The maintained updater through the existing GUI unit preserves that startup mode and service selection. No passive or Native restoration was performed in this successful retry. The window END was reported and the original display lock released normally; documentation follow-up does not hold rollout or the watcher. |
| Remaining native/platform acceptance | INCOMPLETE | No deliberate Post/click, approval ENABLE, synthetic read mark or raw host ACK was submitted. Qt type rollback/removal and package qualification were not performed. Full G3, delivery/click, terminal, voice/video, destructive-review and broader G6 cases remain open. |

Selected records are retained privately under `.ci-build/g6-arch-5c110055/`.
Stable role labels above replace private request UUIDs, service epochs and
process identities; no private project/session names or contents are published.
The installed image SHA-256 values were:

- CLI: `16227b68f443b23677ec9580e192c22d3036825f4795dd1eacddc2290b68d1a1`.
- GUI: `ddc8305714ffc78abbb6f8ba2d90f33a8aadbfa3dd535b0983baca1bdbf96f34`.

These SHA-256 values identify the exact retained evidence bytes:

| Private evidence filename | SHA-256 |
|---|---|
| `step5-verification-summary.json` | `f7356171d048182b00b0f76b99df4c2795e583f980974d16d35eb739d616df91` |
| `preflight-images.json` | `0b0ee2de6af18a8ca10431a54f62072d690ba53e8e047e14d0b0b5cc801d5b43` |
| `status-final-background.json` | `01675ec8c0887c5ede212134dca01db126316d23b51b61b34f132ee69298cd69` |
| `grant-after-resume-result.json` | `d29d6b74f1b20b224d5e530ae22eebf80ce86e767b60a36cec4a3f385be1bb3f` |

### Remaining G3 source work at this run

The whole `.66.5` migration is still incomplete. The preference/organization and
Windows-folder callers are merged through [#870](https://github.com/ufna/zerus/pull/870).
The separate active Next pending-read transfer is wired by
[#973](https://github.com/ufna/zerus/pull/973),
[#992](https://github.com/ufna/zerus/pull/992) and
[#999](https://github.com/ufna/zerus/pull/999).
The remaining Qt/interim source adapters and `migration::Bundle`/`Publisher`
still lack production discovery/capture/publication callers: their presence and
tests do not produce a complete private import/quarantine record.

migrate-u owns this remaining work with desktop-u's handoff seam: discover the
managed sources, freeze and freshly recheck their actual writers/snapshots, then
publish bounded immutable evidence using the existing adapters and publisher.
Preserve original identities and unknown/partial state without replay. This is
several focused source PRs (`.66.5.2.1`, `.66.5.2`, `.66.5.4`), and blocks the
complete one-time-import and upgrade acceptance rows. The completed service
read/approval implementation `.66.3.29` is separate; its closure does not close
G3 or installed acceptance.

### Historical creation source capture

In service mode, the original Next creation worker captures its managed
`creations.json` before its first **service-mode** load, recovery normalization
or journal write. A native-mode run still loads its journal without this capture.
The fixed private `creation-preload-v1` slot contains the durable capture UUID,
original source object/stamp/hash, exact bytes and a read-back receipt. Raw input
is limited to 8 MiB and the existing schema-1 decoder's 128 operations; metadata
leaves are limited to 64 KiB. Nothing lists other source files or starts a second
queue. The original worker proceeds only after the complete capture is synced,
read back and rechecked. A verified missing source is not-applicable, not an
empty imported journal.

Unsupported, oversized, unreadable or changed input keeps that original legacy
owner paused, including Hide/Show/Stop and writes. Its redacted
`retained_source_import.creations_capture` status is visible beside the
independent Qt/Settings report; normal service Retry wakes that same worker.
Failed/partial files remain for review, with the original capture UUID; Retry
never overwrites a partial slot or adopts a newer source version. Connection,
Grant, watch/catalog, ownership Resume and the separate G2 actor stay available.
Do not clear the slot to manufacture a successful report.

On Unix the capture resolves the managed data folder once and pins that canonical
object for capture and the original legacy journal load/writes; symlinks through
home, `$XDG_DATA_HOME` and `.local/share` are supported.
The existing data folder must be owned by the user; a group-writable mode such as
0775 is preserved. The archive slot must remain user-owned and private (0700),
and source/archive leaves remain private, owned, single-link and no-follow. A
`creation_source_private_slot` refusal asks to check archive availability and
restore its private permissions before Retry; `creation_source_data_directory`
asks to check the managed data folder's availability and ownership.
`creation_source_unavailable` asks to check private file access/ownership (0600
on Unix) and review retained partial files before Retry. No permission
or source repair happens automatically.

These are endpoint identity rechecks, not an exclusive namespace hold. Deliberate
same-user rename-and-back between checks is outside this protection. Other users
in a shared writable data-directory group can rename the archive slot away, just
as they can the live journal; private leaves and rechecks do not make 0775 a
hostile-writer exclusion guarantee. Missing/replaced data remains unavailable and
never grants current coverage or effect authority.

After capture, normal live journal writes may resume. The archive is explicitly
historical pre-load data: a later source recheck validates the retained private
snapshot's native identity and raw hash, not today's mutable journal. It is not
Drained, current-source completeness, worker ownership or execution consent.
This exception does not relax the Qt/Settings source leases. The separate
creation report binds this exact capture ID once to the paired canonical service
configuration. It publishes the unchanged creation source and
an explicit historical provenance source through the existing Staged publisher;
its own `creations-retained-import-v1.json` retains the configuration, snapshot
reference, attempt and expected receipt. It never uses the Qt/Settings journal's
hash fields. Attempts/unknown results keep their original ID and are lookup-only.
A complete exact negative permits explicit Retry of the same record after fresh
snapshot validation. A confirmed local callback failure before the final record
write has its own durable `before_record` state: no final record was sent, so
explicit Retry may prepare that same identity again without inventing a receipt.
Missing, partial or lost child output never establishes this state. New records
use journal version 2, which also keeps `recorded_attempt`: before its final frame,
an attempt's callback durably records its receipt and `recorded_attempt`, under
the native lock and against that attempt's exact stored hash. An Attempted record
whose `recorded_attempt` is below its attempt, like a version 1 Attempted record
without a receipt, therefore proves that no final frame was sent, even after a
second metadata failure. Only an explicit Retry recovers it. It first looks up the
original ID: a matching record settles as published, and any other answer stays
lookup-only. It then revalidates the same snapshot and configuration, and fences
the record to `before_record` at the same attempt before reserving the next. The
fence changes the record's hash, so a still-pending original callback can no
longer record or send. A recorded gate, Unknown and every ambiguous state stay
lookup-only; a changed snapshot is quarantined. The native
metadata lock is explicitly unlocked before close, so a forked child's inherited
descriptor cannot prolong it. Retained SDK/helper/pipe resources
keep the shared report admission charged until actual completion. Publication
runs off the UI/legacy worker and never gates Grant, watch/catalog or Resume.
Capture/report success alone does not complete G3 or installed acceptance.

The service-mode, non-replay native local-mark writer also preserves the exact
`attention-marks.json` bytes before its first load/normalization in a separate
`local-read-preload-v1` slot. This historical boundary is specific to local
presentation marks, not the shared host-read queue or `attention-observed.json`.
The original pair order, duplicates and unknown fields remain in the private
snapshot. The existing decoder still refuses more than 20,000 entries in either
map; a larger captured original is retained, never exported after capping it.
Capture failure pauses local-mark mutations and all file writes, including
flush/drop and thread-spawn fallback, while presentation reads still answer.
Review shows `local_read_capture` independently; Retry wakes the same writer.
A loaded model retains editable unsaved changes after a failed file write;
Retry checks the same live file object before saving them.
Later writes check the canonical managed directory and the same live leaf
identity/hash; a changed leaf is never adopted. Every current file is classified
without normalization before a restarted model is installed, even when an older
historical receipt exists.

The separate `local_reads_report` publishes that original snapshot and its
historical provenance through the existing Staged publisher after paired
canonical configuration is known. Its fixed scope and private journal are
separate from the Qt/Settings and creations reports. Review keeps capture and
publication reasons separate. Later local writes do not change the original;
publication rechecks the retained snapshot's private identity and raw hash.
A lost or uncertain result only looks up the original import UUID. Explicit
Retry can repeat only a verified not-committed attempt or a durably proven
failure before the final record, with the same snapshot/configuration/receipt
and a compare-and-swap fence against a late original callback. All groups share
the existing single report admission and original helper/pipe resource charges;
report failure never gates Grant, watch/catalog or Resume.

These are source/component controls. A Staged receipt is historical evidence,
not shared-read delivery or acknowledgement, notification suppression,
current-source completeness, ownership or permission to act. Installed import
and upgrade acceptance remain pending.

The native NewSession owner also preserves `worktree-snapshots.json` in a fixed
`worktree-cache-preload-v1` slot before its first service-mode load or file write.
The exact historical original and capture ID remain unchanged after later cache
writes. Both the original and today's restarted file pass the existing strict
64-record/8 MiB cache decoder before the pruning model runs (raw reads stay
bounded to 16 MiB). Opaque cache hashes never prove current checkout ownership.
Only this legacy cache's read accepts an owned, single-link 0644 Unix source;
group/other writes, links and changed canonical identities refuse capture.
The source is never chmodded. Archive/metadata and replacement cache files stay
private (0600); Windows native ACL/reparse checks are unchanged. The existing
local-disk and ancestor-list requirements still apply; a redirected UNC source
remains unavailable.

A failure pauses only cache file persistence. Fresh folder queries and separately
authorized session creation remain available; dirty in-memory cache data stays
bounded and unsaved until an explicit same-owner Retry passes its original
source checks. Quit refuses late model installation and new cache writes.
Review shows `worktree_cache_capture` separately from drafts, local read marks
and published reports. The separate `worktree_cache_report` publishes only the
committed archive plus its pre-first-load provenance under the closed
`next-worktree-cache-preload-v1` scope and `worktree-cache-retained-import-v1.json`
journal. It binds once to the actual paired canonical configuration, original
capture/import ID and expected record. Later live writes or a paused cache
writer do not replace that historical source. Every publication reopens and
rechecks the private original; it never reads today's mutable cache as evidence.

The existing one-slot report executor and original helper/pipe resources remain
in use. An unknown publication is lookup-only under the original ID. An exact
negative or a fenced definitely-unsent preparation can be retried only explicitly
with that same source/configuration/payload. Missing is not applicable;
unsupported, over-cap, changed or unreadable retained data stays incomplete and
reviewable. Grant/watch/catalog/Resume stay independent. This is component/source
coverage, not current checkout identity, creation/cleanup consent, drain or
installed acceptance; no acceptance row is promoted.

The native draft-store capture from [#1230](https://github.com/ufna/zerus/pull/1230)
also has a separate Staged publisher in
[#1279](https://github.com/ufna/zerus/pull/1279). It reads only the committed
`drafts-preload-v1` receipt, index and original container; it never captures or
substitutes today's drafts. The schema-1 families distinguish their boundaries:

- `next-drafts-preload`: Next's own store, `next-drafts-before-first-job-v1`.
- `next-dev-drafts-preload`: the dev stores, `next-dev-drafts-observed-v1`.
- `qt-drafts-preload`: the Qt store, `qt-drafts-observed-v1`.
- `next-drafts-preload-provenance`: the receipt and each root's boundary,
  presence, entry count and completeness, with `historical: true`,
  `drained: false` and `current_source_complete: false`.

Store families use `draft-preload-entries-v1`; provenance uses
`draft-preload-provenance-v1`. A store family's SHA binds its ordered index
entries, capture ID, container SHA and boundary; individual raw-manifest and
container hashes stay separate. Supported entries carry the unchanged
`sources::draft_checkpoint` body. Other classes retain classification/hash
metadata, with their original bytes kept in the private capture. Attachment
references are stat-only; blob contents are not included. Pages retain the
existing 64-entry/1 MiB bounds; an oversized entry is refused, never truncated.

A missing receipt, a recorded capture failure or an unverifiable capture
publishes nothing and remains required/incomplete; publication Retry does not
replace it with live drafts. An earlier uncaptured Next run omits the Next
family. Its Qt/dev originals or provenance alone may be saved, but the durable
binding keeps `partial: true, complete: false` across Retry and restart.
The fixed `next-drafts-preload-v1` scope and `drafts-retained-import-v1.json`
journal use the same single report admission and original-result lookup rules.
Review keeps `drafts_capture` and `drafts_report` separate. This is source
coverage only: no draft send, blob copy, import, deletion, current completeness,
drain or installed acceptance is established by the report.

## macOS

For a coordinator-approved private click fixture, opt in to actual GUI selection
readback with `ZERUS_CLIENT_ACCEPTANCE_SELECTION=1` and a canonical, owned 0700
`XDG_STATE_HOME`. The main webview records its current page, detail tab and selected
computer/name/run/conversation in `hgs/client-selection-diagnostic.json` below that
private state directory (atomic 0600, at most 2 KiB, timestamp and source commit).
Only one write and the latest pending observation are retained. Match this actual
selection against the original Delivered mapping after the owner's click; a native
intended target or a visible project alone does not prove the selected Activity.
The opt-in is off by default and accepts no navigation, permission or service
command. Readback is observation only and never supplies authority.

Use the signed/notarized Next bundle/cask and the maintained local installer for
the owner's main-account evaluation. **Run Mac first after the source admission
gates merge; GUI/native acceptance may run at any time without a separate owner
window.** Close every opened window, never restart agents, tmux or DeepSeek hosts,
and avoid notification spam. Fresh and upgrade source installs default to Next via
`mac-install.sh`/`packaging/desktop/install.sh` ([#904](https://github.com/ufna/zerus/pull/904));
`HGS_MAC_GUI=client` remains an explicit selection before retirement. Keep the
existing privacy identity, signature, installed path and one verified rollback copy.

| Pending check | How to verify | Coverage / owner |
|---|---|---|
| EXCEPTION — owner accepted 2026-11-03 | Combine the exact candidate's CI install/package checks (bundle/CLI identity, signature/notary and no Qt frameworks/plugins) with the maintained owner-account upgrade. Cask installation must not quit apps or start services. Link both results; do not clear state or claim an empty-account install. | G5 `.66.7.2`, [#871](https://github.com/ufna/zerus/pull/871); macbundle-u |
| [x] Upgrade with agents running — coordinator re-plan | COVERED (coordinator, 2026-11-05) by the maintained owner-account Next→Next queue rollouts `7ae0c2bf6` → `7a60dceeb` → `0f6017973`, with native processes untouched and drafts kept; see the [gate matrix](#deletion-gate-matrix--2026-11-04). Existing Qt users' Qt→Next path is covered by the CI install/package checks and `test_macos_switch_updater`; an owner-account Qt→Next run is waived with the Qt rollback. | `.12.6`, [#871](https://github.com/ufna/zerus/pull/871), `packaging/desktop/update-installed.py` |
| [x] Terminate and Remove worktree (`.66.3.40.4`) | PASS — the installed-pair Terminate-and-Removed positive on the maintained signed pair `a9d5b41f8` (#1504): one original confirmation, Terminate completed, archive retained, folder removed, branch kept, original processes gone; verified by macbundle-u. See the [Mac Terminate-and-Removed record](#recorded-mac-installed-terminate-and-removed-positive--source-a9d5b41f). | `.66.3.40.4`, [#1504](https://github.com/ufna/zerus/pull/1504); macbundle-u |
| [x] One-time import — approved shared coverage | Covered by the coordinator-approved Windows fixture/CI plus INI/plist/Core/CLI evidence above; no owner-GUI swap or Mac-native import claim. Preserve the explicit Windows legacy-list and later-repair limits. | G3 `.66.5.2.1`, `.66.5.3`, [#870](https://github.com/ufna/zerus/pull/870) |
| WAIVED — owner 2026-11-05 | Service registration/readiness: waived by owner 2026-11-05 (no recorded evidence). | `.66.3.27.2`, [#882](https://github.com/ufna/zerus/pull/882); macbundle-u/desktop-u |
| WAIVED — owner 2026-11-05 | Notifications/clicks: waived by owner 2026-11-05 (no recorded evidence). | `.66.3.30.2/.3/.4`, [#913](https://github.com/ufna/zerus/pull/913) signed host, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; [#891](https://github.com/ufna/zerus/pull/891) is extraction only |
| WAIVED — owner 2026-11-05 | Menu bar/lifecycle: waived by owner 2026-11-05 (no recorded evidence). | G4 `.66.6`, `.66.3.31`, inventory native shell/menu rows |
| WAIVED — owner 2026-11-05 | Terminal: waived by owner 2026-11-05 (no recorded evidence). | inventory Mac terminal rows; native terminal contracts |
| WAIVED — owner 2026-11-05 | Voice/video: waived by owner 2026-11-05 (no recorded evidence). | G2 `.66.4.5`, inventory media/privacy rows |
| WAIVED — owner 2026-11-05 | UI/durable parity: waived by owner 2026-11-05 (no recorded evidence). | G1–G4/G7, `.66.3.31/.32/.35/.38` |
| WAIVED — owner 2026-11-05 | Qt rollback is waived by owner 2026-11-05: no Qt rollback or Qt round trip is prepared or run on any platform. | `.66.3.31`, updater rollback and defaults-snapshot checks |

### Recorded Mac native-host subchecks — source `f1833dec`

macbundle-u reported the maintained main-account installation
`f1833decd99e503b4ee48314df298894006c8062`. migrate-u checked the three bounded
summary/receipt/status files below against their supplied hashes. Owner sensory
observations and the private fixture-to-target comparison remain attributed to
macbundle-u; this was not an independent rerun. **Mac acceptance remains open
(`.66.3.30.2.4`); no platform checkbox or #906a merge gate is satisfied here.**

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| One service-host Post | PASS, one synthetic target | One controlled synthetic reply produced one Delivered Post. The retained receipt/native-token hash matched the producer's reserved private run/conversation fixture. The original callback was host/default, one callback with zero GUI callbacks. This does not establish delivery for other targets or a second attempt. |
| Owner banner and click | PASS, owner-reported scope | The owner reported one new banner and one click into the fixture project. **Actual selected Activity remains UNQUALIFIED:** the run exported no exact selected-session readback. Project visibility alone is not exact-target selection proof. |
| Normal revoke/drain | FAILED | The retained status has one pending native Post hold, `notification_cleanup_deadline`, `notification_host_unresponsive`, `drained:false`, `operation_drained:false` and no observed host exit. `background_drained:true` does not establish the missing physical drain. The original revoke remains unsettled. |
| Original withdrawal | UNKNOWN, no replay | The asynchronous Withdraw receipt is `unknown` / `native_error`; it is not a completed withdrawal or a retry grant. Keep its original identity and uncertainty. |
| Exceptional physical cleanup and owner restore | Separate recovery, not normal-drain PASS | The coordinator separately authorized one TERM to the test host and one to the private service, with zero KILLs. The test GUI/host/service were absent afterward; owner LaunchAgent, native agents and rollout agent were untouched. The owner reopened the ordinary installed GUI, Native ownership revision 15, and the watcher resumed. The supervisor, fixture/data and checkout were deliberately retained. Cleanup does not settle the original revoke or unknown Withdraw. |

P0 `.66.3.30.2.8` / [#1210](https://github.com/ufna/zerus/pull/1210) addresses
the native host lifetime and provides an opt-in bounded exact-selection readback.
Its source/component tests are not the required rerun. A reviewed installed
candidate must establish normal drain and actual selected Activity under the
coordinator's next admitted route. Do not replay the original unknown operations.
Import/upgrade, tray, terminal, voice/video and the rest of G6 remain unqualified.

The selected evidence is retained privately by macbundle-u under
`.ci-build/mac-native-acceptance/installed-4u7n-bounded-summary/`. These hashes
identify the checked summary bytes, not public SDK metadata or owner content:

| Evidence file | Bytes | SHA-256 |
|---|---|---|
| `summary.json` | 833 | `d5f29374b8de0a3a894150ba3fabc7366f0839264dec8f08b9508a1c40d2ffed` |
| `selected-receipts.json` | 664 | `95ad49391c527b69082ffea7f7f51868e4acdde8b77a87612f53985a9e97f3ca` |
| `selected-cleanup-status.json` | 406 | `73bcf6e34d938b088268497978f57855e3150d8198502e1edce53aa88581d308` |

### Recorded Mac installed caller trial — 2026-11-05

Macbundle-u reported **BLOCKED** for the coordinator-admitted installed
candidate `69ef4c5fbf9e4d57dde75a017d685f94b49abd8a`. The dedicated
Accessibility runner's consumer-tree inspection reached its bound. Correcting
that ad-hoc runner changed its code identity and invalidated the human grant;
its self-probe returned `trusted=0`. UI actions stopped and no regrant arrived.
The genuine Terminate-with-Remove caller was never exercised.

| Subcheck | Result | Scoped evidence |
|---|---|---|
| Installed-pair Terminate/Remove | BLOCKED, no attempt | Resume, synthetic target/session/worktree, retention, actual Terminate and removal counts were all zero. No capture, confirmation or Terminate UUID was created. No unknown operation was replayed and no folder was manually deleted. This supplies no actual caller positive for `.66.3.40.4` or #1340. |
| Required Native restoration and drafts | PASS, producer-reported restoration scope | The consumer normally quit after a stable held-zero barrier. One clean same-source GUI returned through ordinary no-environment LaunchServices, with the consumer flag absent. A fresh Native takeover showed revision 28 and current identity; the original service stayed passive/serving at the same epoch. All 538 draft records were preserved (22 unsent, 319 form-only, held zero). Normal `--sessions` forwarding succeeded. The local catalog was empty before and after; visual and remote-list usability remain unqualified with AX untrusted. |
| Runtime and retained test artifacts | Settled runtime; artifact cleanup pending | The runner exited and was retired to PID zero, leaving GUI effects unavailable. No synthetic process, worktree or private-service residue remained, and native agents, tmux, DeepSeek and other sessions were untouched. The test app and private evidence remain for the owner to remove the sole TCC entry and for subsequent bounded residue verification. |

The restore proof SHA-256 is
`7424ee05a4502d89b0b14d9b9ca0525d03d8cbc313e0099e7e692f43ee5b6321`;
the draft manifest SHA-256 is
`abe727fb0b175ce5e51dbddc4bb5be0103a6861abfc994090e28256cf74c4f72`.
Macbundle-u retains the bounded source/status/signature artifacts privately.
These hashes record the scoped restoration evidence; no SDK metadata, private
stdin or session body is published here.

The result dependency completed with BLOCKED, not an acceptance PASS.
`.66.3.40.4` stays in progress, its trial child `.66.3.40.4.1` stays blocked,
and #1340 still needs the installed-pair positive. Reviewed source and hosted
native controls from [#1363](https://github.com/ufna/zerus/pull/1363) remain
separate evidence. A future trial requires its own coordinator-designated
candidate/window and result correlation. Platform checkboxes remain unchanged.

### Recorded Mac paired caller and kept-folder trial — source `7ebe7307`

On 2026-11-05, macbundle-u reported an owner-present trial of the maintained,
genuine signed pair `7ebe7307603b6bdc6fe3f73059306e95028732e6`. The runtime
observations and evidence hashes below are producer-attributed, not an independent
rerun by migrate-u. **The genuine caller and original Terminate completed, but
the full Terminate-and-Removed result remains BLOCKED.** No platform checkbox or
Service-default gate is satisfied by this scoped result.

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| Genuine installed caller | PASS, producer-reported scope | The Service consumer observed every original paired helper before stdin and bound the original frontend-request Child. Exactly one native-confirmed disposable target produced one immutable retained Intent, matched to its original configuration, service epoch, run, conversation, ownership nonce and capture. No path, SSH or proof fallback was used. |
| Original Terminate and recovery identity | COMPLETED | Maintained same-original receipt lookup with expected-machine returned completed and the exact archive. Direct native absence checks confirmed the selected original agent and supervisor were gone. The branch, archive identity and restoration source were preserved. This is not an exported LaunchServices GUI/helper exit-code receipt. |
| Folder cleanup | KEPT: 0 removed, 1 kept, 0 unknown | Native membership inspection of one protected process returned EPERM, despite a stable short/unique process identity. Full process information, cwd/root, FD listing and standard-FD vnode probes were unavailable. The folder remained Git-registered and untouched. The unreadable process was not proven to use the folder; the trial applied no manual acceptance or force and sent it no signal. This is a definite keep, not an uncertain deletion. |
| Queues, ordinary restore and drafts | PASS, producer-reported scope | The reported operation/resource queues were idle. The consumer quit normally, and the same app returned through ordinary no-environment launch to current Native ownership revision 44, with the same original service and no headless host. All 341 meaningful drafts were unchanged. Queue-idle status is not a separate physical helper exit-code claim. |
| Full caller/Removed qualification | BLOCKED | No folder was removed. `.66.3.40.4` stays in progress and `.66.3.40.4.1` remains blocked; #1340 still needs its actual installed-pair qualification. The completed caller/Terminate subchecks do not close that dependency. |

The later owner permission-denial decision, mirrored by
[#1499](https://github.com/ufna/zerus/pull/1499), applies only prospectively to
explicit Remove through the reviewed source path and its existing protections.
It does not upgrade or replay this historical KEPT outcome. Fixes-u owns that
runtime/G1 slice; a future qualification uses a fresh original fixture after
reviewed source and coordinator booking.

Macbundle-u retains the bounded evidence privately. Its supplied SHA-256 values
are recorded here for correlation only, without SDK metadata, session bodies,
private input, configuration values or process identifiers:

- Original termination evidence: `823659678d6e8fa8c2c18b3c6fd60183e33bbc1fff415785d61cad0cf5fa60e2`.
- Kept-folder evidence: `86f80a1560936dcdb2faaa1b16009848115dddb2473216a39fa6ec86baad5df2`.
- Native membership evidence: `4b79af5e4ad15f16dff5fe6902dab17ba215a8e935dc5770887a3f00528400a1`.
- Ordinary restore evidence: `e3639400cf8f770684d353da88ded1c0a9e98e8fd5720800739483d2c2ddebae`.

### Recorded Mac installed Terminate-and-Removed positive — source `a9d5b41f`

On 2026-11-05, macbundle-u ran a new coordinator-admitted, owner-present trial
using the maintained signed installed pair
`a9d5b41f88edcaaecd476bdf544b386b0cd5bb66`, reporting clean
`0.47.1-preview.design-polish.a9d5b41`. This candidate includes the original-helper
synchronization in [#1498](https://github.com/ufna/zerus/pull/1498) and the explicit
Remove permission-denial rule in [#1501](https://github.com/ufna/zerus/pull/1501).
The disposable target was fresh; the earlier archived target and its KEPT folder
were not retried. **The installed-pair Terminate-and-Removed positive required by
`.66.3.40.4` passed.** This result does not close the broader Mac platform rows or
flip the Service default.

| Subcheck | Result | Actual evidence and limit |
|---|---|---|
| Installed identity and original configuration | PASS | Build-info, image hashes, local pair, provenance and strict deep signature verification agreed on the frozen source. The rollout watcher was paused. One consumer GUI used the original configuration and owned registration, with its per-launch Service opt-in; the owner's explicit Resume produced current Service ownership with an owner epoch. No private profile, authority fallback or second GUI was used. |
| Exact original confirmation and completed Terminate | PASS, one target and one confirmation | The owner opened the exact fresh target's dialog, checked Remove and pressed Stop and remove folder once. Exactly one immutable retained Intent matched the original configuration, service epoch, capture, run, conversation, checkout nonce, confirmation and request. Its original bytes matched the retained integrity hash; maintained same-original receipt lookup returned completed and the exact archive. The genuine paired caller retained the original helper/registration path; no direct helper substitute or exited-output adoption was used. |
| Native exits and actual folder removal | PASS: 1 removed, 0 kept, 0 unknown | Direct native absence checks confirmed both the selected original agent and its supervisor had exited. The exact removal receipt reported removed with branch_deleted false. The generated checkout was absent and no longer Git-registered; its branch still pointed to the original head, and the same archived run/conversation/ownership identity remained readable with Restore available. No manual folder deletion, manual acceptance, force or effect replay was used. |
| Operation resources | PASS, scoped observations | The exact checkout/removal queue rows were zero. Root reported undrained zero, unsettled zero, idle resource custody, no pending reads and no errors; no transport-marked helper remained under the consumer. After normal Quit and ordinary restoration, the original consumer and headless host were absent. These observations do not export a separate native helper exit-code receipt. |
| Ordinary restore and drafts | PASS | After the owner's normal Quit and a zero-GUI witness, exactly one ordinary no-environment launch restored the same installed app to current Native ownership revision 48. The original desktop service's PID, native birth and epoch were unchanged. Exactly one ordinary GUI remained, the headless host was absent and notification_runtime was null. All 539 draft records matched the pretrial semantic manifest: 22 unsent, 319 form-only, held zero. The branch and archive were retained; the removed checkout stayed absent. |

The acceptance commands did not signal or restart any owner's native agent,
unrelated session, tmux server or DeepSeek host. The new fixture tmux server also
exited naturally with the target: direct native absence checks found no original
target agent, supervisor, fixture server, consumer or headless host remaining.
No tmux stop command was sent. The synthetic repository/branch, archive and
private fixture evidence remain for restoration identity and review. The owner
reported that the Remove checkbox appeared after opening the dialog; no
latency was measured, so this is only an observation for the existing progress
work, not a timing qualification.

This is a new result, independent of the earlier completed BLOCKED dependency
and the `7ebe7307` KEPT result. Historical Unknown and KEPT outcomes retain their
original identities and were not replayed or upgraded. No notification Post,
click, selection readback, media or Qt rollback was performed in this trial; those
rows retain their own recorded status and limits. Qt rollback remains waived by
owner 2026-11-05.

Macbundle-u retains the private records; these SHA-256 values identify their
exact bytes without publishing process identities, configuration values,
private input, SDK metadata or session bodies:

| Private evidence filename | SHA-256 |
|---|---|
| `a9d5-admission-1793898615111027728.json` | `89a3caa77ee06e94058ea782c56895564180736b22f9946ec5fe4be63d3fff63` |
| `a9d5-resume-verify-1793899101694793615.json` | `201bd90f7bff50bed95c1fb8a231fc0fb305867d064d23a019a13239d1f3c88d` |
| `a9d5-termination-observe-1793899581918380684.json` | `f4553785a7a09fc6baaa6fd77ec8e4756fb3593ecd054d2209da8d1f2fd8350f` |
| `a9d5-removed-proof-1793899753232087849.json` | `5cd78523fa8d6f04711044d3bd4742435326955dfe2f81fc2637f8a2d06b16b2` |
| `a9d5-normal-restore-result-1793900050246897530.json` | `dacffcf870db825d2673fd5ee6df064361016a1711946e8e2e3ed9259bb88b90` |
| `a9d5-final-residue-1793900289887254244.json` | `9a1d55c716598f62d8f0ead05e70f9da3616cc5d349c042693d2caf03b00c09d` |

## Installed client cutover on Linux and macOS

**Historical preparation baseline:** the source limitations in this section
describe the earlier prepared candidate. Dated installed records above supply
scoped results; execute remaining cases under the 2026-11-05 admission and owner
session decision. Unrecorded cases remain pending.
Client procedure: `.66.3.31.1.1`, webkit-c; results and checkbox ledger: G6,
migrate-u. Select and record the exact current-integration candidate, never the
Qt-deletion draft. Run macOS first after all source gates below have merged,
without a separate owner window for unattended rows; current Arch execution uses
the 2026-11-05 idle/display admission above.
The installer/service primitives below were checked at `c319a25f` and remain
unchanged at preparation baseline `1729e5c6`; recheck the selected candidate.

### Admission and isolation

Stop before execution until desktop-u's reviewed, authenticated service-mode
ownership grant/rearm (`.66.3.30.11.11`), its actual background native-host/click/drain
integration (`.66.3.30.11.12`) **and webkit-c's client companion
(`.66.3.30.11.11.1`) are merged**. Main-account use and Mac's anytime scheduling do
not waive these gates. Mac needs no separate owner window after they clear;
Arch still waits for the owner's rescheduling. At the preparation baseline:

- `background::Runtime` passes `None` to attention/activation polling and reports
  attention `ready:false`, `code:discovery_only`. Component adapter tests do not
  establish installed notification readiness.
- A retained revoke or handoff marker blocks background restart; complete old
  import evidence reports `delegation_required`. There is no public
  `desktop activate`, `resume` or `handoff` command. Insert desktop-u's actual
  reviewed grant/rearm UI/command sequence into the run record before proceeding,
  including the supported owned-service fault/passive/restore sequence used by
  the cases. If that sequence is unavailable, stop here.
  `desktop startup` is negative exclusion; `owner-status` reports no authority.
  Neither manager restart nor deleting markers can supply the missing grant.

Use the owner's main interactive account and its existing canonical install,
configuration, service and GUI route. Do not create or substitute a separate test
account, or start a second same-UID canonical switch with different config/profile
paths. Same-UID directories do not isolate Linux
`hgs-tray.service`/`hgs-desktop.service`, Mac
`com.hgdev.hgs-tray`/`com.hgdev.hgs-desktop`, or the canonical GUI socket.
A staging prefix, different development app ID, fake manager, altered unit label
or source test is not installed canonical authority.

Use the owner's actual main graphical login's HOME, runtime, bus and temporary
directory. File/profile overrides do not authenticate a frontend. For an Arch
owner session, the coordinator brackets the run with START/END and load snapshots
and the executor holds
`flock -w 1800 /tmp/zerus-arch-display.lock` for the display run, and stops if the
screen locks or sleeps. Never replace the lock or invent display/bus values.
Mac GUI/native acceptance needs no separate owner window after the source gates
clear. Close every opened window, never restart agents, tmux or a live DeepSeek
host, and avoid notification spam on either platform.

### Maintained install and trusted GUI start

Record exact GUI/CLI source, build/artifact identity, installed paths and prior
manager definitions before changing anything. For a main Dev source build,
`ZERUS_DEV_BUILD_NUMBER` must be that exact source's CI sequence. Preserve the
maintained preview identity when using the preview route.

| Platform | Maintained route on the owner's main account | Interpretation |
|---|---|---|
| Linux | Exact AUR/package route for the package row; source evaluation uses `bash ./install.sh`, then `HGS_LINUX_GUI=client bash ./packaging/desktop/install.sh`. | The installer/updater owns replacement, drafts, rollback and service refresh. Root install can update connector/recovery/swarm/voice workers too; it is not a sandbox. |
| macOS | Native checkout: `bash ./install.sh`, then `HGS_MAC_GUI=client bash ./packaging/desktop/install.sh`; the existing remote route is `HGS_MAC_GUI=client ./mac-install.sh "$MAC_ALIAS"`. | The alias must select the owner's main login. The SSH wrapper copies `tmux.conf` over `~/.config/tmux/tmux.conf` and runs `tmux source-file` to reload configuration; it does not restart tmux. Preserve the installed signed pair, existing signing identity and independent runner. |

Let the Mac installer perform `prepare-local-macos.py`, `sign-local-macos.py`
and `update-installed.py` in their maintained order. Register the **installed**
bundled service `hgs`, not a build/staging copy. The independent ordinary runner
may differ after signing; do not require its bytes to equal the bundled helper.
Published bundle setup is explicitly `"$APP/Contents/Resources/bin/zerus-setup"`;
cask installation alone is not setup, registration or readiness. Autostart is a
separate opt-in.

On both platforms, setting `HGS_INSTALL_PREFIX` makes the root installer stage
the CLI there and skip worker/service refresh. The Linux tray uses it for file staging; the
Mac tray ignores it. `DESTDIR` does not redirect these installers; it only
suppresses the root Linux desktop refresh. `HGS_TRAY_RESTART=0` is
not isolation: definitions can still be written and an absent GUI can start.
None of these paths supplies positive registration or a running canonical UI.

For the actual client cases, set exactly `ZERUS_CLIENT_DESKTOP_SERVICE=1` for a
**new trusted GUI process** on the owner's main account. The flag is startup-only;
a second start forwards to the existing process and cannot change its mode.
Record the previous value or absence before setting it. These manager settings
apply to every later unit/agent launched in that login until removed or restored,
not just Zerus. With the original owned launcher present, after the old GUI's
normal draft-safe Quit:

```bash
# Linux, only after owner rescheduling, in the main login with its maintained GUI unit:
systemctl --user set-environment ZERUS_CLIENT_DESKTOP_SERVICE=1
systemctl --user start hgs-tray.service

# macOS, after source gates merge, in the main Aqua login with its maintained GUI LaunchAgent:
launchctl setenv ZERUS_CLIENT_DESKTOP_SERVICE 1
launchctl kickstart "gui/$(id -u)/com.hgdev.hgs-tray"
```

Do not force-restart a running GUI. If that original launcher is absent, G6 must
select the maintained trusted desktop launch path and verify the flag reached
the new process; do not fabricate a second definition. Launching from an
agent-descended shell or clearing `HGS_SESSION`/`HGS_RUN_ID` does not establish
consent or erase ancestry. Confirm the UI is actually in service mode before
any positive effect case. These main-account launch settings do not change the product default.

### Read-only source and manager preflight

`SERVICE_HGS`, `RUNNER_HGS`, `GUI`, `APP`, `CFG` and `STATE` below are the exact
absolute installed selections from that maintained route. On Linux the service
CLI is the installed CLI; on Mac it is the signed bundled helper. The GUI,
service definition and these probes must agree on configuration. Use the
registration preview's actual `readiness.argv` and `readiness.environment`;
preview itself is not registration or writer permission.

```bash
: "${SERVICE_HGS:?selected installed service CLI}" "${GUI:?selected installed GUI}"
: "${CFG:?selected config}" "${STATE:?selected state}"
"$GUI" --build-info
"$SERVICE_HGS" --version

desktop_cli() {
  env HGS_CONFIG_DIR="$CFG" HGS_STATE_DIR="$STATE" "$SERVICE_HGS" desktop "$@"
}
desktop_cli registration --json
desktop_cli status --json
desktop_cli owner-status --json

# Linux manager identity; run only the platform's applicable command:
systemctl --user show hgs-desktop.service --property=LoadState,ActiveState,MainPID,FragmentPath,DropInPaths --no-pager
# Mac Aqua manager identity:
launchctl print "gui/$(id -u)/com.hgdev.hgs-desktop"
```

A new binary's `--build-info` alone does not identify an already-running GUI.
Bind the actual new canonical PID/build/nonce to its original managed launch.
A CLI version string alone is not source/artifact proof. Keep Mac GUI, bundled
service helper and independent runner identities separate. Manager presence,
`ipc_ready` and `writers_enabled` do not mean every background duty is ready;
record lifecycle ABI/configuration/epoch/generation and each component's
`ready`/`code` plus the durable approval opt-in independently. Disabled policy
can be ready; degraded runtime can retain a true opt-in. No JSON readiness field
grants authority.

Use only the installed route's original helper when registration needs action:
Linux helper `--check`, `install [--no-start]`, `session`, and existing-only
`refresh` are in `scripts/install-linux-desktop-service.py` (`--hgs`, optional
absolute `--config-dir`/`--state-dir`). Mac helper takes the installed bundled
binary, `--runner-hgs`, `--check-only`; `--refresh-runner` preserves that backend
during CLI-only updates. `--restart` is only for an actual owned backend change.
Foreign/custom definitions refuse. No-start, headless/Aqua deferral, inactive
refresh or Linux exit 3 (PendingRestart) is not readiness. Unix has no
`desktop registration install/remove/status` command; those are Windows-only.
Never hand-start a second `desktop serve` as a repair.

### Linux background registration opt-in

The passive actor accepts the service-mode GUI's authenticated Grant/Rearm and
journals it without starting background workers. Complete that handoff first;
do not infer it from readiness. Then, from the owner's foreground terminal
**outside Zerus** in the current graphical login, use the maintained helper:

```bash
: "${DESKTOP_HELPER:?installed lib/zerus/install-linux-desktop-service.py from this candidate}"
python3 "$DESKTOP_HELPER" install --hgs "$SERVICE_HGS" \
  --config-dir "$CFG" --state-dir "$STATE" --mode background --json
python3 "$DESKTOP_HELPER" status --hgs "$SERVICE_HGS" \
  --config-dir "$CFG" --state-dir "$STATE" --json
desktop_cli status --json
```

Require the exact configuration and selected/running background mode separately
from fleet/presence/read/notification readiness. Missing/stale native Grant or
retained revoke refuses; perform authenticated frontend Rearm rather than
clearing files or launching a second service. Agent ancestry and `HGS_SESSION`/
`HGS_RUN_ID` refuse even if the environment is stripped in a child. The future
native Settings control uses this same helper contract; an agent cannot perform
the acceptance mode switch on the owner's behalf. Default install is passive;
normal refreshes preserve selection. Mac uses its own maintained route below;
Windows switching remains separate work. Linux results do not qualify them.

For an admitted Linux passive restoration, use `install --mode passive --json`
with the same selections. It durably retains one revoke UUID and
requires the original checkpointed drain before changing/restarting only
`hgs-desktop.service`. Record `last_revoke_request_id` on success. On uncertainty,
`status.transition.request_id` is the original ID; inspect it and resume only
that same passive transition. The native `desktop-revoke-hold-v1` child remains
held through the manager change and passive readiness; its matching release,
actual exit and pipe drain precede completion. Do not substitute a new revoke
UUID or return to background while it is pending. If a new authenticated Rearm
won before acquisition, use the documented explicit
[`cancel-rollback --request-id ORIGINAL_UUID`](linux-distribution.md#select-the-desktop-service-mode)
record-only recovery, then choose a new passive intent explicitly. Cancellation
requires a current Service grant and changes no native revoke, unit or manager.
The helper never restarts the GUI or agents.

### Mac background registration opt-in

Complete the actual service-mode GUI's authenticated Grant/Rearm against the
passive actor first. From the coordinator's genuine non-agent foreground
Terminal context in the current Aqua login, use the installed paired selections:

```bash
: "${APP:?canonical installed paired app}" "${RUNNER_HGS:?independent installed runner}"
MAC_DESKTOP_HELPER="$APP/Contents/Resources/setup/install-macos-desktop-service.py"
python3 "$MAC_DESKTOP_HELPER" "$SERVICE_HGS" --runner-hgs "$RUNNER_HGS" \
  --config-dir "$CFG" --state-dir "$STATE" --mode background --json
python3 "$MAC_DESKTOP_HELPER" "$SERVICE_HGS" \
  --config-dir "$CFG" --state-dir "$STATE" --status --json
desktop_cli status --json
```

Require the signed bundled service image and independent runner to stay selected,
and verify selected/running background mode separately from each component's
readiness. A private pty, stripped markers, SSH agent context or launchd-submit
does not replace the native caller/ancestor check. Missing/stale Grant, retained
revoke, no Aqua or a custom plist refuses; do not repair it by changing files or
starting another service. Only the owned desktop LaunchAgent reloads; neither
GUI nor agents restart. Normal GUI installs and CLI-only refresh preserve mode.

For an admitted Mac passive restoration, use this helper with the same
selections and `--mode passive --json`. It retains one original revoke UUID and
requires its exact
checkpointed runtime/resource drain and the original native hold through
bootout/rewrite/bootstrap and passive readiness. Require the matching release,
actual child exit and both pipe EOFs before transition completion. Record
`last_revoke_request_id`; on uncertainty, inspect `--status --json` and resume
only its original passive transition. Authenticated Rearm is required before
returning to background. No file clearing or new UUID substitutes for settlement.
If a newer Grant won before acquisition, or the original revoke never took
effect, the owner may use the Mac helper's `--cancel-rollback --request-id
ORIGINAL_UUID --json` only with a verified current Service grant. This cancels
registration metadata, with no manager/native ownership action; a later mode
choice is a separate explicit intent. See the [Mac mode contract](macos-distribution.md#explicit-desktop-service-mode).

### Client cases

Use fresh synthetic targets on the owner's main account. Record original exact
computer/session/run/conversation/reply/attention identities privately. Observe
the actual UI and its existing native responses without adding another batch
consumer or editing stores. Missing required observations leave a case BLOCKED
or INCOMPLETE, not inferred from a fixture, local overlay or aggregate count.
Use the selected host's bounded
`hgs inspect "$TARGET_REFERENCE" --read-observation` (or
`hgs @configured-alias inspect "$TARGET_REFERENCE" --read-observation`) for a
current journal-backed main session. Require `read_observation.complete:true`, `freshness:"observed"`,
`source_state:"present"`, its content `generation` and host `observed_at` from
this invocation. Match **both** the top-level and `read_observation.target`
run/conversation to the captured original target; reference resolution alone may
follow a resumed conversation into a different run. The raw `read` fields must
cover the exact captured parts. They are stored by stable owner/conversation;
the observer reports the current run binding separately, not a new delivery
receipt. Retain the generation/time with the result. Missing raw fields, absent
storage, `complete:false`/unknown, changed target or unsupported provider keeps
the host-ACK subcase **BLOCKED**, even after a successful queue transfer. The
reader caps each source/binding file at 16 MiB and shares a one-second cooperative
budget; it does not list other sessions or change the marks. See
[raw shared-read inspection](reference.md#existing-session-snapshots). Do not substitute
an unbounded full listing or the GUI's local overlay.

| Case | Operator sequence | Required observation |
|---|---|---|
| New start and restart | Start the new trusted service-mode GUI through the maintained launcher. Open one synthetic session and a draft. Quit normally, prove its original UI/proxy exit, and reopen by the same trusted route. | One canonical GUI; matching new PID/build/source and service configuration; draft and exact session identities retained. Canonical responder remains, GUI writer Bootstrap/read/approval/notification/presence workers do not start. Closing the window/GUI leaves admitted service duties running; actual background readiness is required separately. |
| Unavailable, Retry and no fallback | Through G6's admitted owned lifecycle route, make only the owned canonical service unavailable; open/refresh the client and selected view, then restore it through the reviewed route and press **Retry now**/the view's Retry. | Explicit unavailable/last-known state, null/incomplete counts preserved, no second service or GUI Fleet/read/approval writer. Healthy sources are not re-baselined by Retry; a halted source/view can recover. New epoch/configuration retires old observation bindings. |
| Frozen read transfer and host observation | Before cutover, create a bounded set of genuine pending marks through the normal main-account UI. Start service mode, observe capture/import and its normal Retry. Keep exact original targets fixed until host observation. | Complete validated source digest/row count, native `pending_read_import` progress, original page/mark IDs and durable new/superseded receipts. `complete:true`/queued means transfer only; `delivered:false` is not host ACK. Through the exact-target `hgs [@configured-alias] inspect` observation above, separately read the current target's raw `read.conversation_id`, `read.reply_id`/`read.attention_signature`; require the same run/conversation and read fields that cover the captured parts. Missing observer or raw fields keeps host acknowledgement BLOCKED even when transfer completed. A changed/superseded target is not ACK of the old part. No GUI overlay or aggregate `pending:0` substitutes for this. Partial/unknown transfer retains frozen source and blocks fresh read delivery; no reseed or duplicate host dispatch. |
| Authenticated enable and confirmation | Record the main account's actual policy; verify default OFF only if that default state is present, otherwise leave that subcase pending without resetting consent. Turn ON only through Settings' existing confirmation; cancel once, then explicitly confirm with no unrelated/private questions eligible. Use one harmless synthetic one-time question to verify the intended flow and its journal, then turn OFF. | Cancel changes nothing. Confirm yields the native authenticated commit for the original logical UUID; ENABLE needs actual runtime/current writer readiness. Generic/agent toggle requests remain refused and do not change policy. No JSON role/confirmed flag, helper wrapper or direct audit Begin/Finish is an alternative. Journal paging/detail stays on the same configuration/epoch/journal/high-water. End with a confirmed durable OFF. |
| OFF while degraded | With confirmed ON and no pending intent, use the admitted owned service lifecycle to reach IPC-connected, stored `automatic_approvals:true`, approvals `ready:false` (for example its documented passive state after restart). Do not revoke/reset policy to manufacture the case. Click OFF. | OFF is available without another confirmation and commits through the authenticated writable route despite degraded/absent workers. ON remains disabled until ready and still requires confirmation. If that state cannot be reached through the admitted source route, leave this case blocked. |
| Uncertain result, lookup, no replay | During a controlled owned-service transition on the main account, submit one Approvals or Updates/Recovery configuration change through its existing UI and retain its original ID. Once the actual UI reports unconfirmed, use **Check result** while transport is unavailable, then after the service returns; check twice. For maintenance, retain the full update-and-recovery configuration; do not submit a second subsystem change while it is pending. | Unavailable lookup stays pending; the logical ID and operation do not change, and no new frontend mutation is sent. A durable receipt settles the original result. Only a later exact negative lookup after confirmed original child exit can settle not-applied; unverified exit remains pending. A fresh opposite intent is allowed only after settlement. If the transition produces only a definitive result/refusal, record that outcome and leave the unknown case incomplete; do not invent an unknown, replay, fake native authority or alter SQL/helper identity to obtain one. |
| Revoke, rollback and return to Next | Waived by owner 2026-11-05: no Qt rollback or return segment is prepared or run. | — |

### Revoke and drain commands

On Linux/macOS, use the appropriate maintained `--mode passive` command above
and retain its original revoke ID. A read-only lookup uses that same original
ID; it does not repeat revoke. Never create a second revoke while the helper
has a pending original transition.
Qt rollback is waived by owner 2026-11-05; no updater rollback command is run.
The remaining commands apply only to an admitted service transition or cleanup.

```bash
# Only if the helper result was uncertain, read its original receipt:
: "${REVOKE_ID:?original ID from the helper transition}"
printf '{"op":"revoke_status","request_id":"%s"}\n' "$REVOKE_ID" |
  env HGS_CONFIG_DIR="$CFG" HGS_STATE_DIR="$STATE" "$SERVICE_HGS" desktop request --json
```

Require matching request/configuration and `accepted`, `durable`, `revoked`,
`checkpointed`. The explicit `applicable:false`, `handoff:none`, `revoked:true`,
`nothing_written` no-op is a separate no-work case; it does not prove that an
active runtime drained. Unknown stops the cleanup sequence. PID disappearance,
a native Stop or `writers_enabled:false` alone is not original-resource drain.
For the normal Next restore, preserve the draft-safe Quit checks and verify the
installed candidate and actual fresh grant/rearm source. The Qt waiver does not
turn an unsettled service cleanup into a successful drain.

### Record and restore the main account

Keep at most 64 KiB of redacted selected-field evidence per step and 1 MiB per
platform record set; these are evidence caps, not protocol changes. Overflow or
missing bytes/IDs is incomplete. Do not dump catalogs, SQL, marks files or
message bodies. Retain private exact IDs while using stable placeholders in
public results. Record:

- Candidate/installed identity and original manager/PID bindings; configuration,
  lifecycle epoch/generation, component readiness and durable opt-in separately.
- Frozen-source completeness and immutable transfer receipts separately from
  fresh host read observations; original mutation IDs and read-only lookups,
  including primary/secondary errors and original child/EOF/wait evidence where
  actually exposed. No missing field becomes a successful zero or empty list.
- Settled revoke, actual resource drain, original session/draft continuity and
  the admitted fresh rearm result. No source-only test counts as installed PASS.

G6 records outcomes in its result ledger; leave every platform checkbox pending
until that actual step passes and the coordinator accepts it. After the admitted
shutdown/revoke/drain sequence, restore the main account's prior launch environment:

- Linux: if previously absent, `systemctl --user unset-environment ZERUS_CLIENT_DESKTOP_SERVICE`;
  if present, `systemctl --user set-environment "ZERUS_CLIENT_DESKTOP_SERVICE=$PREVIOUS_MODE"`
  with its recorded previous value.
- macOS: if previously absent, `launchctl unsetenv ZERUS_CLIENT_DESKTOP_SERVICE`;
  if present, `launchctl setenv ZERUS_CLIENT_DESKTOP_SERVICE "$PREVIOUS_MODE"`
  with its recorded previous value.

These commands affect future launches, not the startup-cached mode of a running
GUI. Do not launch a legacy writer while the service still owns its duties.
Close opened windows and remove only owned test entries through their maintained
teardown after confirmed exit/drain. If teardown is unavailable or uncertain,
preserve the main account's state, registration and frozen source and report the
blocker; never hand-delete units, markers, stores or a GUI copy. Do not stop
agents or unrelated services.

## Windows — va-kids

Use **CI-built Windows artifacts from the exact tested commit**, never Arch
binaries. Installed acceptance uses the owner's main interactive account and
canonical route, with no separate test account; keep staged artifacts in a unique
private folder on that account. Earlier component checks under other profiles
below retain their original scope and are not main-account acceptance. Close
opened windows and remove test registrations unless testing the final install.
Do not touch unrelated desktop/apps/settings.

| Pending check | How to verify | Coverage / owner |
|---|---|---|
| EXCEPTION — owner accepted 2026-11-03 | Combine CI portable-ZIP/per-user-installer install/package checks with the owner-account upgrade. Verify `zerus-windows-x86_64/hgs-tray.exe`, adjacent `hgs.exe`, connector/ConPTY, WebView2 readiness and no Qt DLLs/plugins. Native Windows PC acceptance remains deferred. | G5 `.66.7.3/.4`, [#875](https://github.com/ufna/zerus/pull/875); fleet-c |
| [x] Upgrade with agents running — fixture profile | PASS (14/14, second run after a harness failure) on integrate `861e0baf`; see the [va-kids `861e0baf` record](#va-kids-unattended-rows-on-861e0baf--2026-11-05). The earlier Qt ZIP→Next upgrade passed 25/25 on `faa4d324` ([record](#va-kids-rows-on-the-latest-integrate-artifacts-owner-decision-4)). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | `.66.7.3`, `.66.6.15`; fleet-c/fixes-u |
| [x] One-time import — fixture profile | PASS (16/16, twice) on integrate `af07efb6`; see the [va-kids 0.48.0 record](#va-kids-rows-for-0480-continued--2026-11-05) and the shared import evidence above. Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | G3 `.66.5.2.1`, `.66.5.3/.10`, [#870](https://github.com/ufna/zerus/pull/870) |
| [x] Service registration/readiness — fixture profile | PASS (12/12) on integrate `861e0baf`; see the [va-kids `861e0baf` record](#va-kids-unattended-rows-on-861e0baf--2026-11-05). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | `.66.3.27.3`, `.66.3.31`, [#912](https://github.com/ufna/zerus/pull/912); fixes-u/desktop-u |
| [x] Standard-user passive registration | Install, status, idempotent reinstall and removal passed on an existing non-administrator console token; see the evidence below. Actual login triggering and service readiness remain in the preceding unchecked row. | `.66.8.1`, [#912](https://github.com/ufna/zerus/pull/912) review follow-up; fixes-u |
| [x] Windows frontend native authentication subchecks | Native process/ancestry and request-evidence checks, six canonical-pipe cases and a separate departed-launcher GUI ancestry probe passed; see the scoped evidence below. This does not check off a durable policy change or whole UI parity. | `.66.3.37`, [#931](https://github.com/ufna/zerus/pull/931), [#955](https://github.com/ufna/zerus/pull/955); fleet-c |
| [x] Notifications/clicks — owner-present | PASS with the owner present on #1475's head `8cf223aa` (13:12–13:20 MSK). H2: the owner saw the banner and clicked it; the click reached the service and the GUI selected exactly the target session. H3: with the GUI quit, the owner clicked the entry in the notification center (Win+N); exactly one GUI cold-started on the same session. Withdrawal (H4) passed earlier. See "Owner-present H2/H3" in the [va-kids 0.48.0 record](#va-kids-rows-for-0480-continued--2026-11-05). The H2 row's strict journal count saw two delivered attempts (likely the read-mark withdrawal), recorded there. | `.66.3.30`, `.66.6.15`, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; fixes-u |
| [x] Tray/lifecycle — fixture profile | PASS (12/12) on integrate `861e0baf`; see the [va-kids `861e0baf` record](#va-kids-unattended-rows-on-861e0baf--2026-11-05). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | [#874](https://github.com/ufna/zerus/pull/874), `.66.6.15` |
| [x] Terminal — fixture profile | PASS (9/9) on integrate `861e0baf`; see the [va-kids `861e0baf` record](#va-kids-unattended-rows-on-861e0baf--2026-11-05). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | `.52`, `.66.6.15`, Windows native contracts; fixes-u |
| [x] Voice/video — documented refusal | PASS for the documented visible refusal (2/2): local Windows voice refuses visibly, on `faa4d324`; see the [va-kids latest-artifacts record](#va-kids-rows-on-the-latest-integrate-artifacts-owner-decision-4). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | G2 `.66.4.5`, inventory voice/media rows |
| [x] UI/durable parity — fixture profile | PASS (20/20) on integrate `861e0baf`, and also within the owner's 2026-11-05 waiver; see the [va-kids `861e0baf` record](#va-kids-unattended-rows-on-861e0baf--2026-11-05). Private fixture-profile evidence (owner decision 4); the row reruns once on the final release candidate (`zerus-5wmh.66.8.1.1.6`). | G1–G4/G7, `.66.3.31/.32/.35/.37/.38` |
| WAIVED — owner 2026-11-05 | Qt rollback is waived by owner 2026-11-05: no Qt rollback or Qt round trip is prepared or run on any platform. | `.66.3.31`, `.66.7.3`; desktop-u/fleet-c |

### Recorded Windows registration subcheck — 2026-11-02

fixes-u reported one lifecycle case with eight real CLI invocations, using source
`e2c80a0e6f6ddff83e2ad48a59e8e7c9be0d263f`, CI run
[41359593881](https://github.com/ufna/zerus/actions/runs/41359593881), attempt 1,
job `125007003118`, artifact `13105895095` (`windows-shell-loader`). The staged
`cli/hgs.exe` was 50,289,152 bytes with SHA256
`013f899e4708b8a87f181b32486b3414c71f17501042bef148e29fc792aa6372`.
The retained report matches that source and binary identity.

The original console logon token had `effectiveAdministrator=false` and no
Administrators SID in its complete groups, including deny-only membership;
this was a standard user rather than an unelevated administrator. The task was
initially absent; install registered it in READY state, reinstall changed
neither registration nor the staged CLI, and removal restored absence.
fixes-u confirmed child waits, task/temporary launcher absence and removal of
the private test profile and stage; no native agents were touched.

The local enable marker remained absent. Final service status was not running,
with writers disabled, lifecycle unavailable, IPC not ready and no PID, epoch
or generation. This covers passive registration only. Login triggering, service
start/readiness, background work, presence, notifications and GUI remain pending.
The separate CI first-status 15-second timeout remains a failed CI result;
this PC subcheck does not make that workflow green.

### Recorded Windows frontend authentication subchecks — 2026-11-02

fleet-c supplied retained reports for source
`7c0c7c043c55d194bc66a897bf980228541eaa81`, CI run
[41375310707](https://github.com/ufna/zerus/actions/runs/41375310707), native job
`125054215361`, artifact `13111379237`. The recorded CI core suite passed 1213
tests with three existing ignored tests, and all six frontend cases passed;
the native job still failed the separate known 15-second registration-status
check. This is not an overall green CI result.

The exact artifact's private va-kids run used console session 1, native 64-bit
processes and a token reporting `effectiveAdministrator=false`. Its retained
report passes 11 process checks, six request-evidence checks and six frontend
cases. These cover same-user agent names/markers, stale birth/configuration/
fingerprint and over-limit environments. The canonical-pipe fixture uses the
production listener/cache and original-child runner: its positive case returns
`desktop_background_unavailable` with `accepted:false` after authentication;
negative cases return `frontend_authentication_refused`. Policy and writers
remain false in both service-status observations.

The caller/helper/SYSTEM ancestry reached a query-only access-denied boundary
(error 5); that does not prove a different SID, as clarified in #955. A separate
read-only native adapter probe admitted the genuine GUI after its original
launcher exited, while the original GUI remained live and responsive. It did
not issue a request or mutation permit for that GUI. The retained cleanup report
confirms child waits, an empty owned job, task absence and removal of 60 private
stage/profile entries; fleet-c reported original-child exit and final wait 0.

No additional standalone `.66.3.37` native adapter/evidence case is identified
by these reports and its current contract. The existing UI/durable-parity row
still needs the actual confirmed UI-to-ready-service commit/result round trip
under `.66.3.31/.32`; the authentication fixture stops before that effect.
Real logon, notifications, media, agent-preserving upgrades and whole UI parity
remain pending in their own rows. Packaging receipts are separate evidence.

The notification checks require the service's actual lifecycle/adapter wiring,
not only the merged ledger, discovery or host primitives. Exercise old unread
replies during first startup (no stale-notice burst), a new reply on a later
catalog page after focus expires, and a new reply during a policy baseline.
Record the accepted conservative first-baseline omission window from
[#914](https://github.com/ufna/zerus/pull/914); native presentation and click results
still need their own platform checks above.

### Recorded Windows installer acceptance — 2026-11-04

win-accept-cl ran the #906b installer acceptance on va-kids for integrate head
`0fca73f54579db2882657d56ca9e08ca399792c8`, CI run
[41783655673](https://github.com/ufna/zerus/actions/runs/41783655673), attempt 1.
Every job of that run passed, including Quick checks and the CI gate. Admission
used the Windows package job `126420804566` and the native job `126420804715`.
Artifact `13301922603` (`windows-x86_64-<source>`, SHA256
`c995126af2874080e3ff7fbb76531573caab491bd8fe6c16b496b5e1f8ef6790`) contains:

- the portable ZIP, 36,155,383 bytes, SHA256
  `6296e44790cddaef14146c4a9cc8fa2d2c1316e981894e288b4024f3180da1ff`;
- the per-user installer, 24,515,324 bytes, SHA256
  `d5107b4d8e4c7177df46ff795e60305996baab8032d5a29420fdc8fddd99de36`;
- `hgs.exe`, 36,961,792 bytes, SHA256
  `04ddfbaa02d7090164842ba3251626cd3d64117b3bd3cd830372b1de97f2e53a`.

The installer payload has 1,106 files, `desktop: tauri` and version 0.47.0, with
no Qt DLLs or plugins.

Both cases used the owner's standard console account in session 1 with an
unelevated token. The console was unlocked; this was checked before each case.
Kaspersky stayed active and unchanged: it logged no events and its quarantine did
not change. A one-shot SYSTEM task started each case through `WTSQueryUserToken`.
Each case used a fresh private stage and fixture profile, its own `HOME`, config
and state, and the real per-user registration namespace, which each case first
verified absent. Every product call was waited until it physically exited, with
its output fully captured.

**Setup case — PASS, 16/16.** A clean per-user `setup.exe` install wrote the
exact payload and `installation.json`. It registered the passive task in READY
state and the Toast AUMID/COM keys without starting the service or creating
durable service state. The staged stable CLI matched the CI `hgs.exe` bytes. The
Start Menu entry targeted the installed UI, and login stayed opt-in. A
same-source upgrade with the explicit login choice added only the UI
`--background` Run value and kept the backend registration. UI-only uninstall
kept the task, stable CLI and Toast identity. The maintained removals then left
the task and every Toast key absent. Seeded config, drafts and session files
survived every step.

**Legacy case — PASS, 22/22.** A clean portable ZIP extraction kept the winget
layout (`hgs-tray.exe`, `hgs.exe`, connector, ConPTY). Its GUI loader reported
the exact source, and its maintained commands registered only passive backend
metadata.

The Qt starting point was a CI-built Qt portable ZIP of
`c7189d936e1743ed5109a59b5351aaa4eed5391e` (Qt 6.10.3, 615 files, SHA256
`af133299dff0b39a94db0b985a0c13285736eca9815ed513085793e75960f442`), extracted
into its own folder. Released Windows builds were only ever portable ZIPs, so the
upgrade followed the documented route: the Next installer went into its own
folder. That left the Qt folder byte-identical (615/615) and installed no Qt
files; seeded config, drafts and session files survived.

The installed Tauri loader reported the exact source. The first start opened its
own visible WebView2 window; the retained screenshot shows the Sessions screen
and one connected machine. The native tray menu (Open Zerus, No remote machines,
Open in, Quit) quit the app within the bound. UI-only uninstall and the
maintained removals then left the namespace absent.

An earlier attempt installed `setup.exe` over the extracted Qt ZIP folder
instead. The installer correctly does not sweep a folder it does not own, so
the Qt files stayed mixed with the Next payload. The owner decided that the
installer must refuse such a folder (`zerus-5wmh.66.8.1.11`).
The installed `uninstall\payload.txt` has CR CR LF line endings when CI packages
it on Windows. The same ordered path list matched; the installer reader is
unaffected (`zerus-5wmh.66.8.1.10`).

Cleanup after each case removed its temporary task, stage and fixture profile.
It also removed the Explorer tray-icon record that the test GUI left under the
owner's `NotifyIconSettings`. A final read-only check found no Zerus task, stage,
registration, shortcut, notification setting or process. A cold standard-user
status preflight of `1ba301c5` (CI 41675738074) the same morning also passed:
`--version`, `desktop registration status` and `desktop notification-registration
status` returned structured results in 33, 112 and 38 ms. That closed the
CLI-only hang scope of `.66.3.27.3.2`.

**Owner-account row.** The step-0 inventory found no Zerus or Qt installation and
no agent CLI on the owner's account. The owner-account upgrade is therefore
recorded under the 2026-11-03 clean-install exception; it was not exercised on
the main account.

**Not covered by this record:** upgrade with agents running, notifications and
clicks, terminal, voice/video, UI/durable parity and Qt rollback. The other rows
stay unchecked; Qt rollback is now waived by owner 2026-11-05. This record ticks
no broader checkbox.

### Earlier va-kids row evidence (owner decision 4)

These rows ran on va-kids on 2026-11-04, in private fixture profiles, before the
owner accepted private-profile evidence (owner decision 4). They are recorded
here as earlier evidence under that decision. They tick no checkbox; the same
rows are rerun on the latest integrate artifact in the approved order: voice,
terminal, upgrade, readiness, tray and lifecycle, UI parity (Qt rollback is
waived by owner 2026-11-05).

Each row used CI artifacts of its exact source and a fresh private stage and
fixture profile. It ran in the owner's console session 1, started by a one-shot
SYSTEM task, and ended with teardown, cleanup and a residue check.

- **Upgrade with live agents: PASS.**
  - Row `up-4` at `0fca73f5` (CI run 41783655673, artifact 13301922603): 23/23.
  - Row `p14-up-1` at `73a9f785` (CI run 41831170444, artifact 13325617629): 25/25.
  - The starting point was the CI Qt portable ZIP and its stable CLI, with a
    live fixture agent session that answered.
  - Starting the Next GUI restaged the exact Next CLI. The same session, run and
    conversation survived, and no host, ConPTY or agent process restarted.
  - The upgraded CLI delivered to the same conversation, which answered. The
    Next GUI showed the turns from before and after the upgrade in one session.
  - A round trip followed. The Qt-era install restaged its own CLI, read the
    state Next had written and delivered to the same conversation. Starting
    Next again restaged the exact Next CLI, with the same session and no
    process restarts.
  - No desktop service owned the fixture, so no revoke or drain took part.
- **Terminal: partial.** Row `terminal-9` at `0fca73f5`.
  - The attach client ran with an exact non-ASCII session name and path.
  - Paste through the console reached the live session.
  - The session stayed live after its own Windows Terminal window closed, and
    reattach showed the same pane with the pasted text.
  - Not covered: resize, PowerShell compared with Windows Terminal, and exact
    selection through UI Automation.
- **Voice and video: PASS for the documented refusal.** Row `voice-5` at
  `0fca73f5`: local Windows voice refuses visibly.
- **UI walk: partial.** Row `walk-1` at `0fca73f5`: every rail screen rendered,
  and the fixture session showed its reply. The full inventory walk and a
  durable frontend request round trip were not covered.
- **Service readiness: FAILED then.** Rows `readiness-1` to `readiness-7` at
  `0fca73f5`.
  - The task-started service was IPC-ready with the UI closed, and it admitted
    the service-mode GUI with current ownership.
  - No maintained route then selected background mode.
  - Settings > About now offers the first background choice (`zerus-yfbd.1`,
    [#1398](https://github.com/ufna/zerus/pull/1398) and
    [#1399](https://github.com/ufna/zerus/pull/1399)). Its qualification on
    integrate `347c8f1c` (firstchoice 35/35, attention ready through the real
    Task Scheduler path) is recorded in `zerus-yfbd.1` and `zerus-emphh`. The
    readiness rerun goes through that choice.

### va-kids rows on the latest integrate artifacts (owner decision 4)

These rows reran in the approved order on 2026-11-05, in private fixture
profiles, under the same rules as above. Each row had its own fresh stage,
teardown, cleanup and residue check. A guard removed the test AUMID's
notification-settings key only when a stage had created it, and changed no other
app's settings. Rows A–C used integrate `faa4d324` (CI run 41944519612,
artifact 13380445725). Rows D–H used integrate `58cce17f` with #1441 (CI run
41951499758, artifact 13382023595).

- **A, voice and video: PASS for the documented refusal** (2/2). Local Windows
  voice refuses visibly.
- **B, terminal: PASS** (9/9).
  - A Windows Terminal attach ran with a non-ASCII name and path. Paste reached
    the live session.
  - Resizing the Windows Terminal window resized the attached pane (120x30 to
    128x33).
  - The app selected exactly this session through UI Automation.
  - The session stayed live after its terminal client exited, and reattach
    showed the same pane.
- **C, upgrade with live agents: PASS** (25/25). This is the Qt ZIP to Next
  upgrade and round trip described above, on `faa4d324`.
- **D, service registration and readiness: PASS** (12/12).
  - The registered task runs as the interactive user with the least privilege,
    never LocalSystem.
  - Settings > About > "Use background mode…" settled background mode.
  - With the UI closed, background mode was IPC-ready with writers enabled, and
    fleet, presence, reads and attention were ready.
  - The named-pipe server is the engine of the registered task's only instance,
    in session 1, as this user. The live fixture session is listed with no GUI.
- **E, tray and lifecycle: PASS** (12/12).
  - Close to the notification area works. A second start shows the first
    instance. Quit from the app's own tray menu works.
  - Login start uses the documented `--install-login-item` and
    `--remove-login-item` route; Windows has no Settings toggle for it.
  - Draft-safe exit: an unsent draft typed through UI Automation was on disk
    after Quit and was restored exactly on the next start.
  - A background start shows no window but owns its tray icon.
- **Historical F, Qt rollback: rollback PASS; the return was BLOCKED without a restart.**
  Repeat/return acceptance is now waived by owner 2026-11-05.
  - After background mode, `desktop revoke --reason rollback` settled for the
    exact request and configuration, and writers went off. The CI Qt ZIP
    restaged the Qt-era CLI, which reached the same session, run and
    conversation and got an answer. The draft stayed on disk.
  - Returning while the revoked service still ran from the renamed-aside Next
    image left the person with no sessions in either GUI mode
    (`zerus-5wmh.66.3.40.3.4`, blocked by `.66.3.40.3.3`).
  - After a fixture-only simulated sign-out/in (stopping only that task
    instance, then one Run), the full return passed 32/32. The service was
    passive with `explicit_handoff_required`, the default GUI was admitted,
    the draft and the Qt-era turn were back, and writers stayed off.
- **G, UI/durable parity: PASS** (20/20).
  - The UI Automation walk matched the CLI's catalog, session and versions.
  - One composer Send through the ready service reached the same conversation
    and was answered. The route was direct, because Windows does not advertise
    the Linux-only deferred send ABI.
  - Automatic approvals stayed off. With the UI closed, the service alone
    stayed ready.
  - Findings: `zerus-5wmh.92` (session rows have no accessible name, on Windows
    and Linux), `zerus-5wmh.66.3.31.14` and `zerus-5wmh.66.3.31.15`.
- **H, notifications:**
  - **H1, mandatory question: BLOCKED.** The service cannot notify a question
    that precedes the first conversation (`zerus-5wmh.66.3.30.14`).
  - **H2, unread reply: Delivered PROVEN; banner BLOCKED, cause undetermined.**
    The journal and our AUMID's History show exactly one post. Do not disturb
    was off, the foreground was not full-screen and the per-app setting was
    Enabled, yet UI Automation saw no banner surface.
  - **H3, cold GUI click: deferred** to an owner-present console run.
  - **H4, withdrawal: PASS.** After `hgs read` on the exact reply, the service
    removed its own History entry within about 1.2 s and retired the mapping.
    Nothing opened or launched afterwards.

### va-kids rows for 0.48.0, continued — 2026-11-05

The same rules apply: private fixture profiles, a fresh stage per row, and
teardown, guard, cleanup and a residue check after each. The sources are
integrate `68b2c426` (CI run 41977902432, artifact 13394048639) and integrate
`af07efb6` (CI run 41989037251, artifact 13399282081).

- **Toast registration removal after a rollback revoke: PASS** on `68b2c426`
  (`zerus-5wmh.66.3.30.8.5`, #1453). The documented sequence has four steps: the
  rollback revoke for R, ending only the running task instance, `registration
  remove`, and `notification-registration remove` with the same R. It removed
  the Toast keys with no manual removal and kept the desktop database's bytes
  and mtime: 11/11 after a passive history, 15/15 twice in background mode.
- **H2 and H3 with the owner present: not observable, because the service
  refused both posts.** On `68b2c426`, the posts at 10:21 and 10:25 MSK were
  refused before any banner, and the owner saw none. Unattended diagnosis on
  `68b2c426` and `58cce17f`:
  - `ToastNotifier.Setting` for `com.hgdev.hgs-tray` failed with HRESULT
    `0x80070490` (element not found). The backend read it before any Show and
    recorded `permission_denied`.
  - The same `58cce17f` build had delivered at 03:58–04:11Z. The failure
    persisted after the owner restarted va-kids, with a fresh per-user
    notification service.
  - An AUMID without a package identity must show one toast before `Setting`
    works, so checking it first deadlocked. The fix is #1475
    (`zerus-5wmh.66.3.30.8.7`, P1). Status hides the refusal code:
    `zerus-5wmh.66.3.30.15`.
  - On #1475's exact head `8cf223aa` (CI run 41996514139, artifact
    13402753346) the unattended diagnosis passed 13/13. `Setting` read
    `0x80070490` at the settled baseline and **Enabled** after the post. Exactly
    one user post was Delivered, and its History entry was the only one; no
    bootstrap scratch entry remained.
  - The four-step Toast removal then passed 11/11. A fresh stage after it read
    **Enabled** at the baseline and delivered one post (13/13), so the removal
    did not make Windows forget the AUMID. What did so on va-kids that morning
    remains unknown.
  - **Owner-present H2/H3 on `8cf223aa`, 13:12–13:20 MSK.**
    - H2: one History entry 1.5 s after the post. The owner saw the banner and
      clicked it, the click reached the service, and the GUI selected exactly
      the target session.
    - H3: the GUI had been quit first. One delivery and one History entry
      followed. The owner did not see the banner and clicked the entry in the
      notification center (Win+N) instead. Exactly one GUI cold-started and
      selected the same session.
    - `Setting` read Enabled after each post. No bootstrap scratch entry
      remained, and the owner saw no brand-only banner.
    - The row's strict count failed for H2: the journal rose by two delivered
      attempts. Its counters include withdrawals, and the click's selection
      marks the reply read, which withdraws the toast. That is the likely
      second attempt, but this row kept no database copy to prove it.
  - **One cold start wrote to the owner's real profile, and it was removed.**
    The H3 click launched the GUI through Windows COM activation with the
    console user's real environment, because activation does not pass the
    fixture's profile redirection. It wrote Zerus app data into the real
    `AppData\Roaming\com.hgdev.hgs-tray`, `AppData\Local\com.hgdev.hgs-tray`
    and `AppData\Local\hgdev`. Those three folders were removed after checking
    that every item was created during the stage, and the residue check was
    then clean. Every later cold-start row runs the same guarded cleanup.
- **One-time import, G3 scope: PASS** (16/16, twice on `af07efb6`). Before Next
  first started, the fixture held Qt sources: a Qt INI and a Qt main draft in
  the redirected `APPDATA`, and a legacy per-computer project alias.
  - The "Upgrading from Qt" notice showed once. **Got it** was saved, and the
    next two starts showed no notice.
  - The Qt INI and draft kept their bytes and times. The Qt draft was not
    restored, and no Next draft carried it.
  - The legacy project list and the shared account profiles were unchanged.
  - Observed, not accepted: the legacy alias did not appear in Next's Projects.
    Projects showed "Project sync unavailable: The saved Qt project
    organization could not be read…" and "Project sync is not set up on this
    computer yet…". The Migration report said no report was available. The
    seeded INI held no real Qt organization, so the first message may come from
    the seed. The owner wants projects to migrate; Windows support is being
    sized.
- **Service refresh after a CLI replacement: PASS** (20/20 on `af07efb6`,
  `.66.3.40.3.3`, #1467), with a **model** of maintained staging. Staging
  restages only changed bytes, so the row moved the running CLI to the exact
  maintained aside name `hgs.exe.old-<seconds>-<8 hex>` and placed an identical
  copy with a new file ID.
  - The original service reported `manual_refresh_required`, drained.
    Settings > About offered **Refresh background service…**, and its separate
    native dialog was confirmed through its own button.
  - The original exited, and one new service with a new epoch ran in
    background mode. After close and reopen, ordinary startup authenticated
    it: writers on, service ownership, no refresh group.
  - Nothing was granted, rearmed, reinstalled or toggled.
  - An earlier fixture with a non-maintained aside name was refused
    (`refresh_scope_changed`), as the staging contract requires. The hidden
    native reason is `zerus-5wmh.66.3.40.3.3.2`.
- **F, Qt rollback: waived by the owner.** The F results above stay as history.
  The Settings observation of the first `af07efb6` F run is withdrawn. It never
  reached Settings > About, because the rail's Settings icon has no accessible
  name, so its "no refresh offer" result proved nothing.

### va-kids unattended rows on `861e0baf` — 2026-11-05

win-accept-cl ran these rows between 14:53 and 15:14 MSK, in private fixture
profiles. win-accept-cl2 reviewed the raw results and wrote this record. The
source is integrate `861e0baff28d4f354eeaedcbcbc9387b27ecaaec`, CI run
[42010395529](https://github.com/ufna/zerus/actions/runs/42010395529) attempt 2:
package job `127208000281`, native job `127207945053` and artifact `13408459971`
(portable 0.47.0, SHA256
`b7819cdb35197793db36877e698d9718266c60f56d9e181f22c8602534cba3bb`). In attempt 1
of the same source, the native job failed three `WindowsTaskMode` tests. Attempt 2
passed with no source change, and the flake is `zerus-yuwgz`.

Each row had its own fresh stage, followed by teardown, the notification-settings
guard, cleanup, the guarded real-profile cleanup and a residue check. The
real-profile cleanup found nothing to remove after any stage. The `importonce` and
`importbad` rows ran in the same batch and are recorded in the shared import
evidence above.

When recorded, no checkbox changed, because these are fixture-profile runs and
not main-account acceptance. On 2026-11-05 the coordinator ticked the Windows rows
on this private-profile evidence (owner decision 4); the release-candidate rerun
below still applies. The coordinator accepted `861e0baf` for now on 2026-11-05. The five rows
rerun once on the final release candidate (`zerus-5wmh.66.8.1.1.6`), with a smoke
of the Windows pointer drag from #1488 and #1491, which this source does not
contain.

- **Service registration and readiness: PASS** (12/12, stage `d3588a16`).
  - Settings > About > "Use background mode…" settled background mode. The
    registered task runs as the interactive user with the least privilege, never
    LocalSystem.
  - With the UI closed, the service was IPC-ready with writers enabled, and fleet,
    presence, reads and attention were ready.
  - The named-pipe server is the engine of the registered task's instance, in
    session 1, as this user. The live fixture session is listed with no GUI.
  - Not covered: a real logon trigger. The row started the task itself, so that
    stays with batched owner question 6.
- **Tray and lifecycle: PASS** (12/12, stage `0f71abd0`).
  - Closing the window keeps Zerus in the notification area. A second start exits
    and shows the first instance. Quit from the app's own tray menu exits.
  - The login opt-in registers only the UI background command, and the opt-out
    removes it. A real login start was not exercised (question 6).
  - An unsent draft was on disk after Quit and was restored in the same session
    composer on the next start.
  - A background start shows no window, owns its tray icon and quits from it.
- **Terminal: PASS** (9/9, stage `92b1dc95`).
  - A Windows Terminal attach ran PowerShell with the exact non-ASCII session name
    and path. Paste reached the live session, and resizing the window resized the
    attached pane (120x30 to 128x33).
  - The session stayed live after its terminal client exited, and reattach showed
    the same pane with the pasted text. The app selected exactly this session
    through UI Automation.
  - Not covered: a console outside Windows Terminal, and the refusal of
    unsupported remote paths.
- **UI/durable parity: PASS** (20/20, stage `ab5b0e72`).
  - The UI Automation walk covered Projects, Machines, Accounts, Sessions and
    About. About showed `Zerus 0.47.0 (861e0ba)` and `hgs CLI 1.56.0`. The Projects
    match is the empty case: the CLI catalog has no entries and the UI lists only
    the default General.
  - One composer Send through the ready service reached the same conversation and
    was answered, with the run and conversation unchanged. The route was direct,
    because Windows does not advertise the deferred send. The sent turn appeared
    from durable history, and the composer was empty afterwards.
  - Automatic approvals stayed off. With the UI closed, the service alone stayed
    ready.
  - Not covered: the complete inventory walk, including tasks, folder mapping and
    bounded details, and the refusal of the generic and agent approval toggles.
  - Seen again: the UI showed the service as not ready on Projects and Machines
    before settling (`zerus-5wmh.66.3.31.15`), and session rows have no accessible
    name (`zerus-5wmh.92`).
  - New: the rail shows `0.1 Dev` while About shows 0.47.0 (`zerus-y48q0`), and the
    "Upgrading from Qt" notice appears in a fixture with no Qt data
    (`zerus-eo2gd`).
- **Upgrade with live agents: PASS on the second run, after a harness failure.**
  - **First run: FAILED, stage `fbebc9ed`, 15:07 MSK.** Its six checks passed, then
    the driver stopped with `Private archive destination already exists`. The cause
    is harness ordering, confirmed in the driver: this batch ran the `setup` row
    first, which had already expanded the Next ZIP into the folder where `upgrade`
    expands it, and the driver refuses an existing destination. Earlier upgrade
    stages ran without `setup`. The row stopped before it started the Next GUI, so
    this run says nothing about the upgrade either way.
  - **What that run left behind.** The fixture's stable CLI stayed Qt-era, so
    teardown failed: that CLI has no `desktop registration status` command. Cleanup
    then refused to remove the stage while the desktop service task existed. The
    residue check listed the service task, the Toast keys, the stage, the fixture,
    and the fixture's host, ConPTY and agent processes.
  - **Recovery, same stage, 15:10 MSK.** A recovery-only row restaged the exact Next
    CLI (2/2). Teardown then passed 4/4, cleanup removed the stage and fixture, and
    the residue check was clean.
  - **Second run: PASS (14/14), fresh stage `4b5525f3` without `setup`, 15:12 MSK.**
    - The start was the CI Qt portable ZIP (`c7189d93`, 0.47.0, CLI 1.56.0) with a
      live fixture session that answered.
    - Starting the Next GUI restaged the stable CLI to the exact Next build. The
      ZIP's own GUI was admitted with no native takeover refusal and bound itself
      to the staged CLI.
    - The same session, run and conversation survived. The host, ConPTY and agent
      processes kept their PIDs and creation times.
    - The upgraded CLI delivered to the same conversation and it answered. The GUI
      showed the pre-upgrade and post-upgrade turns in the same session, then quit
      from its tray menu.
  - Not covered: the installer route, because this row uses the ZIP; a saved draft
    or account surviving, because the row checks neither; and a real model, because
    the agent is the pinned fixture agent. This row has 14 checks where the earlier
    run had 25 because the Qt round trip is waived.

## Deletion PR merge gate

**Owner waiver and gate state (2026-11-05).** Every platform row above is ticked
with its recorded evidence or marked "WAIVED — owner 2026-11-05": the owner waived
every row that had no recorded evidence (see the decision at the top). **The
Deletion PR merge gate is satisfied for #906a and #906b** on that record. The
Windows rows still rerun once on the final release candidate
(`zerus-5wmh.66.8.1.1.6`).

The coordinator records per-platform acceptance and any exceptions against the
inventory; unmet G1–G5/G7 responsibilities remain blockers. The owner approved
this split on **2026-11-03**:

- **#906a (Linux/macOS):** ready for source review once final, based on current
  integration; merge only when **both macOS and Linux acceptance records are
  complete**, including the clean-install exception's CI/package and owner-upgrade
  evidence. Remove the `arch` and `mac-desktop` Qt jobs, their runner/sampler,
  Linux/macOS Qt build/start/rollback selections and macOS Qt assembly/install
  inputs. Keep the shared sampler/shard helpers for the retained optional Windows
  Qt source tests until #906b; the retired sampler CI regression is removed. This also retires the tests tracked by `.66.21` and `.66.24`.
- **#906b (Windows/tree):** retain as a draft until **va-kids acceptance**. Remove
  the remaining `tray/` C++/CMake/tests, Windows Qt installer/pin/notices, shared
  Qt archive installer, comparison programs and obsolete documentation. Audit
  non-CI rollback/build dependencies before deleting the tree. Historical immutable
  Qt ZIP validation and old-client update compatibility remain supported.

Coordinator rollback decision (2026-11-03), clarified in R3: a verified replaced
Next becomes `desktop-previous`. Every displaced Qt/unclassified rollback image
gets its own unique entry under `desktop-qt-legacy/qt-*/`; several older Qt
versions may coexist. They are never deleted or offered as rollback. The settings
export stays at its compatible path. If an archive move fails, the image remains
in a named retained location (or both named locations if the move is uncertain),
and the verified Next update still completes its draft/process/provenance checks.
No archive error grants automatic deletion or a retry of an unknown effect.

Windows PC testing is deferred and is not a merge gate for #906a. The ready state
of #906a is not an acceptance result or a merge authorization. Each part must be rebased on the final installer/consumer/notification/packaging PRs and pass
source/privacy, workflow/security, package and retained migration/updater tests.
Audit final artifacts for Qt/KF6/WebEngine requirements and dangling source paths.
Keep `hgs`, `HGS_*`, service/app IDs, installed paths, user data, migration readers,
CLI/connector/ConPTY and immutable published assets unchanged. Keep legacy update
feed fields and sealed-candidate retry templates while old clients require them.
Remove each part only at its gate above; the coordinator owns merge and rollout. This document authorizes no install,
service cutover, credential operation or deletion of user data.

## Deletion gate matrix — 2026-11-04

Owner: win-accept-cl, for the coordinator (`zerus-5wmh.66.8.1`). This matrix maps
every applicable gate row to arch, Mac and va-kids at integrate `7d2b9b93`. It
promotes no checkbox; a cell is complete only with a full per-platform record.

Each cell has three parts:

- **Have** is the evidence recorded so far, including today's runs: the Windows
  installer cases on `0fca73f5` (setup 16/16, legacy 22/22, #1339); desktop-u's
  Arch window on `0fca73f5` (`zerus-5wmh.66.8.3`); and macbundle-u's Mac window
  on `b72dea32` (`zerus-5wmh.66.3.30.2.4`).
- **Auto** is a route that needs only existing access, with no new grant, install,
  security or settings change.
- **Owner** marks what needs the owner's presence or a new grant. Those items are
  batched into the owner questions below.

Shared routes the cells refer to:

- **Qt round trip (arch, Mac): dropped.** The owner waived Qt rollback on every
  platform on 2026-11-05, so no round trip is prepared or run. The coordinator
  re-planned the rows that used it (2026-11-05):
  - upgrade with live agents and the owner-account half of the clean-install
    exception are covered by the maintained owner-account Next→Next upgrades;
  - Linux/macOS one-time import uses the approved shared fixture/CI evidence
    above, including the Windows private-profile rerun; no owner-GUI swap.
- **Synthetic session.** A disposable target that the coordinator or the test
  creates, as in the Arch and Mac windows today; never the owner's agents. On
  va-kids it is a fixture agent in a private profile, because the owner's
  Windows account has no agents.
- **Windows UI driving.** A temporary SYSTEM task starts a helper in console
  session 1 with the user's token. That helper uses UI Automation and SendInput at
  the same integrity level, including on Toast buttons, with no new grant.
- **Arch input.** The session user already has an ACL on `/dev/uinput`, so raw
  uinput pointer events (no package install) and `spectacle -b` screenshots can
  click a banner or menu in a scheduled display window. Injecting input into the
  live Plasma session is listed as owner question 3. XTEST into XWayland stays
  forbidden; a private Xvfb with XTEST remains available for fixture walks.
- **Arch audio.** `espeak-ng` already exists and can speak into a temporary
  PipeWire virtual source (`pw-cli`/`pw-loopback`), removed afterwards. Choose
  that source explicitly in Zerus. `pw-record` on the sink monitor proves that
  non-silent playback reached the output.

| Gate row | arch | Mac | va-kids |
|---|---|---|---|
| Clean install / package (exception) | **Covered, approved exception:** exact743ba9f41 CI42014169811 green, Arch package/artifact13410438906 and disposable-home install checks verified; maintained owner-account upgrade is recorded separately. No clean-account install claim. **Owner:** none. | **Have:** clean frozen `b72dea32` pair/seal verified today. **Auto:** the exact candidate's CI signed/notarized bundle check. Owner-account upgrade half: covered by the maintained owner-account Next→Next upgrades in the next row (coordinator, 2026-11-05). **Owner:** none. | **Have:** PASS. CI install/package checks plus setup 16/16 and legacy 22/22 on `0fca73f5`. The owner-account half is recorded under the exception: no Qt install exists. **Owner:** none. |
| Upgrade with live agents | **Have:** today all 63 original native PID/births survived GUI Quit/restart; rollouts keep 63–64 agents. Both are Next→Next only. **Covered (coordinator, 2026-11-05):** the maintained owner-account Next→Next upgrades with live agents. On arch, linux-shell-u's owner session moved `7a60dceeb` → `8e74aa884` with the same Codex/hgs/tmux/job/pane, exact draft bytes and 57 native processes untouched (#1472). On the Mac, the queue rollouts moved `7ae0c2bf6` → `7a60dceeb` → `0f6017973` with native processes untouched and drafts kept. Existing Qt users' Qt→Next path is covered by the CI install/package checks and the updater suites (`test_tray_update`, `test_macos_switch_updater`); an owner-account Qt→Next run is waived with the Qt rollback. **Owner:** none. | **Have:** rollouts keep agents (Next→Next). **Covered (coordinator, 2026-11-05):** the maintained owner-account Next→Next upgrades with live agents. On arch, linux-shell-u's owner session moved `7a60dceeb` → `8e74aa884` with the same Codex/hgs/tmux/job/pane, exact draft bytes and 57 native processes untouched (#1472). On the Mac, the queue rollouts moved `7ae0c2bf6` → `7a60dceeb` → `0f6017973` with native processes untouched and drafts kept. Existing Qt users' Qt→Next path is covered by the CI install/package checks and the updater suites (`test_tray_update`, `test_macos_switch_updater`); an owner-account Qt→Next run is waived with the Qt rollback. **Owner:** none. | **Have:** legacy case, Qt ZIP→Next installer in its own folder, with seeded config, drafts and session files preserved; no agents. **Auto:** a private profile running a fixture-agent session and ConPTY job through the Qt ZIP's `hgs.exe`, upgraded to Next; verify the same session, run, conversation, job and draft. **Owner:** question 5. |
| One-time import | **Covered by approved fixture/CI evidence; no owner-GUI swap.** Windows variants18/18 each on861e0baff, plus real-shaped INI/plist/Organization controls and taskview-cl real-CLI recovery. Windows legacy-list refusal and later-repair limits remain explicit above. | **Covered by the same approved shared evidence; no owner-GUI swap.** Plist controls are included; no Mac-native import claim. Windows receipt and precise limits are attached above. | **Have:** none. The owner account has no Qt registry or INI state (inventory 06:10Z). **Auto:** none on the main account; seeding a legacy source there would manufacture state. **Owner:** question 5. |
| Service registration / readiness | **Have:** registration/readiness and custom refusal on 8e74aa884; actual owner-present closed-GUI presence/catalog PASS on 743ba9f41, same backend and fresh heartbeat. **Auto:** a new UI-closed durable-read receipt remains. **Owner:** no further step for this run. | **Have:** today the service and host were ready, drain and revoke settled, and wrapper `.4.11` failed closed (macbundle-u fix). **Auto:** inspect the `zerus-setup` LaunchAgent; foreign-definition refusal against a private directory; UI-closed duties on a synthetic session. **Owner:** none. | **Have:** passive registration [x] (11-02); today install/status/remove, cold activation refusal and no service start. **Auto:** start the registered task in session 1; check the pipe identity and readiness with the UI closed; remove it afterwards. **Owner:** the real logon trigger needs a sign-out/sign-in (question 6). |
| Notifications / clicks | **Have (8e74aa884):** `.66.3.42` PASS: one Post, one physical Open-session click, exact Wayland/native callback, one relaunch and exact target selection (desktop-u). **Auto:** stale/withdrawn refusal. **Owner:** lock/idle remains separately pending; no repeated Post/click in this completed slot. | **Have:** a Post plus an owner click into the project (`f1833dec`; selection unqualified; drain FAILED). Today a Post was Delivered and drain settled; Withdraw is Unknown; no click. **Auto:** Post, drain and withdraw rerun after `.4.11`; selection readback (#1210). **Owner:** the click and selection (question 1); lock/idle (question 6). | **Have:** none. The GUI routes #1170/#1174/#1175 are merged. **Auto:** a service-owned Toast from a synthetic session, clicked through UI Automation, with stale and withdrawn refusal and a cold-GUI launch (win-notify-cl plan `.66.3.30.8`). **Owner:** lock/idle (question 6). |
| Tray / menu bar / lifecycle | **Have (8e74aa884):** native menu Quit, original exit, maintained background start, second-instance routing and primary Activate PASS; three nonempty drafts retained and backend unchanged. **Auto:** remaining visual panel/menu and lock/idle subcases. **Owner:** none for the recorded checks. | **Have:** none. **Auto:** reopen and second instance through `open`; maintained Quit. **Owner:** menu-bar clicks, Services, Hide and Show All (question 1). | **Have:** legacy case first-start window, own tray menu (Open Zerus / No remote machines / Open in / Quit) and native Quit (fixture). **Auto:** on the main install, second instance, background start, draft-safe exit, and setting/removing the login opt-in, all through UI Automation. **Owner:** the real login start (question 6). |
| Terminal | **PASS, exercised owner cases:** existing-agent upgrade continuity; on 743ba9f41 a real shell in a space/non-ASCII folder, exact argv/cwd, prompt and paste, PTY resize52×103 →45×71. Only that original shell/folder/catalog links cleaned. **Auto:** unrun test-shell reattach. **Owner:** no further step. | **Have:** none. **Auto:** none; driving Terminal.app needs AX. **Owner:** the expected Apple Events consent and GUI driving (questions 1 and 2). | **Have:** none. **Auto:** a fixture session through Windows Terminal, PowerShell and ConPTY on paths with spaces and non-ASCII; exact selection through UI Automation; paste and resize through SendInput; continuity after GUI exit. **Owner:** none. |
| Voice / video | **Have (8e74aa884):** three-second voice preview, owner-confirmed audible playback/discard; five-second video and working controls in a different session, owner-confirmed discard. **P2:** darker/lower-quality playback `zerus-6ioin`, cause unassigned. **Auto:** transcription/exit-recovery subcases. **Owner:** completed for this run. | **Have:** none. **Auto:** none; consent state is unknown, and synthetic input needs a virtual audio driver. **Owner:** question 2. | **Have:** none. Voice for local Windows sessions is unsupported by design (installation docs). **Auto:** check through UI Automation that the refusal is visible. **Owner:** none. |
| UI / durable parity | **Have:** byte-exact upgrade draft preservation; later owner image confirms the unsent draft; bounded current catalog and one exact host read ACK on 8e74aa884; approvals stayed OFF. **Auto:** the remaining inventory walk and current authenticated policy/result cases under normal guards. **Owner:** no further instruction for the completed segment. | **Have:** none. **Auto:** read-only service and catalog queries. **Owner:** the GUI walk (question 1). | **Have:** first-start window only. **Auto:** a UI Automation walk against a fixture profile with a synthetic session; one frontend request through a ready service to its durable result. **Owner:** none. |
| Qt rollback / reimport | Waived by owner 2026-11-05 on every platform; no Qt round trip. | Waived by owner 2026-11-05. | Waived by owner 2026-11-05; question 5 no longer covers Qt rollback. |
| Open failures | `zerus-6ioin` (P2 camera playback quality). The earlier `.66.3.41` capture failure and `.66.3.42` launch failure have successful scoped 8e74aa884 reruns above; this does not close unrelated remaining acceptance cases. | `.66.3.30.2.4`: click and navigation owner-pending (AX untrusted, loginwindow frontmost); wrapper `.4.11`. | `.66.8.1.11` (P2, installer refuses a non-owned folder), `.66.8.1.10` (CR CR LF), both win-accept-cl; `.66.8.1.12` (P4 log line). |

### Batched owner questions

1. **Mac clicks.** Either one window in which the owner unlocks the Mac and
   performs the listed clicks (one banner, menu-bar items, a short UI walk), or
   Accessibility for one dedicated, revocable test runner, so macbundle-u can
   drive them.
2. **Mac media and terminal consent.** Microphone, camera and Apple Events
   consent for `com.hgdev.hgs-tray`, if not already granted. Then either one
   spoken phrase with the owner confirming playback, or permission to install a
   virtual audio driver for synthetic speech.
3. **Arch input injection.** May tests inject uinput pointer events into the live
   Plasma session during coordinator-scheduled display windows? This uses the
   existing ACL; no new grant. If not, the owner performs one physical click per
   window.
4. **Audible playback.** Does a PipeWire (or WASAPI loopback) monitor capture of
   non-silent output count as playback evidence, or must the owner listen once?
5. **Windows main account.** It has no Qt install, Qt settings or agents. Accept
   private-profile evidence (Qt ZIP plus a fixture-agent session) for the upgrade
   and import rows (Qt rollback is waived by owner 2026-11-05). Or authorize installing Qt 0.47.0 plus an agent
   on that account; that needs the owner's agent login.
6. **Logon and lock.** On all three machines, the lock/idle notification subcases
   need a lock and an owner unlock. The va-kids logon-trigger rows need one
   sign-out/sign-in. Either the owner does these once per machine, or they stay
   recorded as owner-pending.
