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


Owner: `zerus-5wmh.66.8.1` (G6). Basis: the
[retirement inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md),
especially its responsibility tables, migration contract and G1–G7 gates.
Owner scheduling decision (2026-11-02): installed acceptance uses the owner's
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
one-time import, service readiness, notifications, rollback or genuine default-OFF
checks. Staging prefixes and fake service managers remain component evidence.

macOS automation may run without a separate owner window under the existing
rules. Ask the owner to perform any new microphone/camera, Apple Events, screen
lock/unlock or authentication consent; never approve privacy prompts for them.
Voice/video quality and any sensory result not observable in the retained
record require their observation. The Arch display checks wait for the owner's
rescheduled window; nondisplay source/package evidence can be prepared meanwhile.
Windows PC execution waits for its separate acceptance. Neither this scheduling
nor the exception permits clearing state, creating a replacement account or
restarting agents, tmux or a live DeepSeek host.

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

**GUI/installed acceptance is deferred until the owner reschedules it**, even
when the source gates have merged. Then use the owner's main account and the
maintained canonical AUR/package or `tray/install.sh` route. Staging prefixes are
file checks only, not installed acceptance. Do not install a test binary by hand.

| Pending check | How to verify | Coverage / owner |
|---|---|---|
| EXCEPTION — owner accepted 2026-11-03 | Combine the exact candidate's CI install/package checks (including Qt/KF6/WebEngine absence and GUI/CLI identity) with the maintained upgrade on the owner's main account. Link both results; do not clear state or claim an empty-account install. | G5 `.66.7.1`, [#867](https://github.com/ufna/zerus/pull/867); linux-shell-u |
| [ ] Upgrade with agents running | From the last Qt release, keep a known terminal/native session and an unsent draft open; record exact session/run/conversation identities. Upgrade through the maintained installer/updater, reopen that same session and verify identity, jobs, terminal and draft survived without a restart. | [#886](https://github.com/ufna/zerus/pull/886), G2 `.66.4`, G6 `.66.8` |
| [ ] One-time import | Apply the 2026-11-05 G3 scope above: projects/accounts, existing Next values, untouched Qt sources, ignored Qt runtime state, retained Next pending markers and the once-only upgrade notice. Restart twice: no reseed or replay. | G3 `.66.5.2.1`, `.66.5.3`, [#870](https://github.com/ufna/zerus/pull/870) |
| [ ] Service registration/readiness | Inspect the generated user unit and `hgs desktop status --json`: one owned service, independent background readiness, graphical-session environment imported. Close Next; catalog/details, presence and durable read delivery continue. Custom units refuse overwrite. | `.66.3.27`, [#886](https://github.com/ufna/zerus/pull/886), [#898](https://github.com/ufna/zerus/pull/898), `.66.3.31` |
| [ ] Notifications/clicks | With Next closed, produce one scoped attention event; get one Freedesktop alert and badge, click into the exact run, then test stale/withdrawn click refusal and Wayland activation. Lock/idle does not invent activity. | `.66.3.30.1/.3/.4`, [#940](https://github.com/ufna/zerus/pull/940) in-process service Linux adapter, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; [#894](https://github.com/ufna/zerus/pull/894) is extraction only; linux-shell-u/desktop-u |
| [ ] Tray/lifecycle | Check panel icon, primary click, menu, background start and second-instance routing; closing windows keeps service duties, Quit flushes drafts and does not stop agents. | G4 `.66.6`, `.66.3.31` |
| [ ] Terminal | Open an existing session and a folder terminal with spaces/non-ASCII paths; check exact argv, resize, paste and reattach after client exit. Never create a worktree by opening a dialog. | inventory “Terminal and process lifetime”; G7 `.66.2`, native terminal contracts |
| [ ] Voice/video | Explicitly choose microphone/camera; record, preview, cancel and transcribe a short synthetic message. Hide/Escape stops capture, private draft survives exit, and an uncertain delivery is not resent. | G2 `.66.4.5`, inventory “Voice, video and speech providers” |
| [ ] UI/durable parity | Exercise each inventory responsibility with its target owner: bounded Activity/archive/search/tasks pages, organization writes, creation/recovery, read marks and manual questions. Use the [installed client cutover runbook](#installed-client-cutover-on-linux-and-macos) for service and intent cases. Approval opt-in starts off; generic/agent callers cannot enable it. | G1–G4/G7; [#884](https://github.com/ufna/zerus/pull/884), `.66.3.32`, `.66.3.35`, `.66.3.38` |
| WAIVED — owner 2026-11-05 | Qt rollback is waived by owner 2026-11-05 ("откат на Qt не нужен совсем"): no Qt rollback or Qt round trip is prepared or run on any platform. | `.66.3.31`, `tray/update-installed.py`; desktop-u |

Linux notification acceptance also requires the root service attachment
(`.66.3.30.11.2`, desktop-u) and activation/click handoff to the service-mode UI
(`.66.3.30.12`, linux-shell-u). The merged adapter's component fixtures do not
complete these routes or the native checks with Next closed. Keep this row pending
until the coordinator's maintained owner route verifies readiness, presentation,
click routing, stale/withdrawn refusal and Wayland behavior.

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
`mac-install.sh`/`tray/install.sh` ([#904](https://github.com/ufna/zerus/pull/904));
`HGS_MAC_GUI=client` remains an explicit selection before retirement. Keep the
existing privacy identity, signature, installed path and one verified rollback copy.

| Pending check | How to verify | Coverage / owner |
|---|---|---|
| EXCEPTION — owner accepted 2026-11-03 | Combine the exact candidate's CI install/package checks (bundle/CLI identity, signature/notary and no Qt frameworks/plugins) with the maintained owner-account upgrade. Cask installation must not quit apps or start services. Link both results; do not clear state or claim an empty-account install. | G5 `.66.7.2`, [#871](https://github.com/ufna/zerus/pull/871); macbundle-u |
| [ ] Upgrade with agents running | Upgrade last Qt via the maintained updater; verify exact running session/run/conversation, terminal/jobs and flushed drafts survive. Keep `com.hgdev.hgs-tray` and `~/Applications/hgs-tray.app`; no native-agent/tmux restart. | `.12.6`, [#871](https://github.com/ufna/zerus/pull/871), `tray/update-installed.py` |
| [ ] One-time import | Apply the 2026-11-05 G3 scope above. Projects/accounts and existing Next values remain; original plist/store/draft files stay untouched. Verify ignored Qt operations, retained Next pending markers and notice acknowledgment across restarts. | G3 `.66.5.2.1`, `.66.5.3`, [#870](https://github.com/ufna/zerus/pull/870) |
| [ ] Service registration/readiness | Through explicit `zerus-setup`, inspect the Aqua `com.hgdev.hgs-desktop` LaunchAgent and actual `desktop status`; close Next and verify background duties continue. Foreign/custom definitions are refused. | `.66.3.27.2`, [#882](https://github.com/ufna/zerus/pull/882); macbundle-u/desktop-u |
| [ ] Notifications/clicks | With Next closed, post one service-owned alert through the signed host; check badge, exact-session click, stale click refusal and lock/idle behavior. Existing notification permission remains attached to the public app identity. | `.66.3.30.2/.3/.4`, [#913](https://github.com/ufna/zerus/pull/913) signed host, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; [#891](https://github.com/ufna/zerus/pull/891) is extraction only |
| [ ] Menu bar/lifecycle | Check icon/menu, reopen, Dock/menu-bar policy, second instance, Services/Hide/Show All and draft-safe Quit; agents and service remain independent. | G4 `.66.6`, `.66.3.31`, inventory native shell/menu rows |
| [ ] Terminal | Open exact session/folder in the configured Terminal path, with quotes/spaces/non-ASCII; accept only the expected Apple Events consent. Check resize/paste and session survival after UI exit. | inventory Mac terminal rows; native terminal contracts |
| [ ] Voice/video | Exercise real microphone/camera consent, short preview/transcription, cancel/hide and exit recovery. Check public privacy identity; never import an old device ID or replay uncertain delivery. | G2 `.66.4.5`, inventory media/privacy rows |
| [ ] UI/durable parity | Apply the same complete inventory responsibility walk as Linux, including bounded details/tasks, archived reads, creation recovery and default-off approvals with authenticated frontend-only enablement. Use the [installed client cutover runbook](#installed-client-cutover-on-linux-and-macos). | G1–G4/G7, `.66.3.31/.32/.35/.38` |
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

## Installed client cutover on Linux and macOS

**Preparation only; no installed case below has been executed or accepted.**
Client procedure: `.66.3.31.1.1`, webkit-c; results and checkbox ledger: G6,
migrate-u. Select and record the exact current-integration candidate, never the
Qt-deletion draft. Run macOS first after all source gates below have merged,
without a separate owner window; the Linux installed run waits for the owner to
reschedule Arch acceptance.
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
directory. File/profile overrides do not authenticate a frontend. Arch GUI
acceptance remains deferred. When the owner reschedules it, the coordinator
brackets that run with START/END and load snapshots, holds
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
| Linux | Exact AUR/package route for the package row; source evaluation uses `bash ./install.sh`, then `HGS_LINUX_GUI=client bash ./tray/install.sh`. | The installer/updater owns replacement, drafts, rollback and service refresh. Root install can update connector/recovery/swarm/voice workers too; it is not a sandbox. |
| macOS | Native checkout: `bash ./install.sh`, then `HGS_MAC_GUI=client bash ./tray/install.sh`; the existing remote route is `HGS_MAC_GUI=client ./mac-install.sh "$MAC_ALIAS"`. | The alias must select the owner's main login. The SSH wrapper copies `tmux.conf` over `~/.config/tmux/tmux.conf` and runs `tmux source-file` to reload configuration; it does not restart tmux. Preserve the installed signed pair, existing signing identity and independent runner. |

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
Use the selected host's bounded `hgs inspect "$TARGET_REFERENCE"` (or
`hgs @configured-alias inspect "$TARGET_REFERENCE"`) for a current journal-backed
main session. Require `read_observation.complete:true`, `freshness:"observed"`,
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
| [ ] Upgrade with agents running | Upgrade the last Qt install in its owned folder; verify exact native session/run/conversation and ConPTY jobs, saved drafts and accounts survive. Stale Qt payload cleanup must stay inside that installer-owned folder. | `.66.7.3`, `.66.6.15`; fleet-c/fixes-u |
| [ ] One-time import | Apply the 2026-11-05 G3 scope above: project/folder mapping and shared accounts, untouched INI/registry/draft sources, ignored Qt runtime state, retained Next pending markers and a once-only notice. No replay or inherited approval consent. | G3 `.66.5.2.1`, `.66.5.3/.10`, [#870](https://github.com/ufna/zerus/pull/870) |
| [ ] Service registration/readiness | Check opt-in registration in the interactive user session, never LocalSystem/Session 0; verify named-pipe identity and independent background readiness with UI closed. Remove temporary logon entries. | `.66.3.27.3`, `.66.3.31`, [#912](https://github.com/ufna/zerus/pull/912); fixes-u/desktop-u |
| [x] Standard-user passive registration | Install, status, idempotent reinstall and removal passed on an existing non-administrator console token; see the evidence below. Actual login triggering and service readiness remain in the preceding unchecked row. | `.66.8.1`, [#912](https://github.com/ufna/zerus/pull/912) review follow-up; fixes-u |
| [x] Windows frontend native authentication subchecks | Native process/ancestry and request-evidence checks, six canonical-pipe cases and a separate departed-launcher GUI ancestry probe passed; see the scoped evidence below. This does not check off a durable policy change or whole UI parity. | `.66.3.37`, [#931](https://github.com/ufna/zerus/pull/931), [#955](https://github.com/ufna/zerus/pull/955); fleet-c |
| [ ] Notifications/clicks | In the real desktop, receive one service-owned toast/badge, click to exact session, reject stale clicks and check lock/idle state without manipulating unrelated apps. | `.66.3.30`, `.66.6.15`, [#907](https://github.com/ufna/zerus/pull/907) ledger, [#914](https://github.com/ufna/zerus/pull/914) discovery; fixes-u |
| [ ] Tray/lifecycle | Check tray/menu, background/second-instance routing, WebView2 window, draft-safe exit and opt-in login start; remove temporary startup values and shortcuts. | [#874](https://github.com/ufna/zerus/pull/874), `.66.6.15` |
| [ ] Terminal | Check PowerShell/Windows Terminal/ConPTY paths with spaces/non-ASCII, exact session selection, resize/paste and live-session continuity. Exercise supported operations only; unsupported remote paths must refuse clearly. | `.52`, `.66.6.15`, Windows native contracts; fixes-u |
| [ ] Voice/video | Verify actual supported capture/preview/transcription/cancel paths and privacy consent; unsupported paths refuse visibly. An unavailable release-required inventory capability blocks acceptance rather than counting as a skip. | G2 `.66.4.5`, inventory voice/media rows |
| [ ] UI/durable parity | Run the complete inventory responsibility walk, especially tasks, folder mapping and bounded details. Verify an actual confirmed frontend request through a ready service to its durable result/query; approvals remain default-off and generic/agent toggles refuse. UI-only mode has no competing GUI Fleet. | G1–G4/G7, `.66.3.31/.32/.35/.37/.38` |
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
clicks, terminal, voice/video, UI/durable parity and Qt rollback. Those rows stay
unchecked, and this record ticks no broader checkbox.

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
- **F, Qt rollback: rollback PASS; the return is BLOCKED without a restart.**
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
  - The cold-started GUI ran with the user's real environment, because Windows
    COM activation does not pass the fixture's profile redirection. It wrote
    Zerus app data into the console user's real profile. Those three folders
    were removed after checking that every item was created during the stage,
    and the residue check was then clean.
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

## Deletion PR merge gate

The coordinator records per-platform acceptance and any exceptions against the
inventory; unmet G1–G5/G7 responsibilities remain blockers. The owner approved
this split on **2026-11-03**:

- **#906a (Linux/macOS):** ready for source review once final, based on current
  integration; merge only when **both macOS and Linux acceptance records are
  complete**, including the clean-install exception's CI/package and owner-upgrade
  evidence. Remove the `arch` and `mac-desktop` Qt jobs, their runner/sampler,
  Linux/macOS Qt build/start/rollback selections and macOS Qt assembly/install
  inputs. This also retires the tests tracked by `.66.21` and `.66.24`.
- **#906b (Windows/tree):** retain as a draft until **va-kids acceptance**. Remove
  the remaining `tray/` C++/CMake/tests, Windows Qt installer/pin/notices, shared
  Qt archive installer, comparison programs and obsolete documentation. Audit
  non-CI rollback/build dependencies before deleting the tree. Historical immutable
  Qt ZIP validation and old-client update compatibility remain supported.

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
  - the one-time import runs as an unattended private-profile fixture.
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
| Clean install / package (exception) | **Have:** source-install identity and no Qt/KF6/WebEngine in the dependency closure (11-02, `a1023d4`); today's `0fca73f5` source install has clean source identity. Neither is a package transaction or a clean-account install. **Auto:** the exact candidate's CI Arch package job. Owner-account upgrade half: covered by the maintained owner-account Next→Next upgrades in the next row (coordinator, 2026-11-05). **Owner:** none. | **Have:** clean frozen `b72dea32` pair/seal verified today. **Auto:** the exact candidate's CI signed/notarized bundle check. Owner-account upgrade half: covered by the maintained owner-account Next→Next upgrades in the next row (coordinator, 2026-11-05). **Owner:** none. | **Have:** PASS. CI install/package checks plus setup 16/16 and legacy 22/22 on `0fca73f5`. The owner-account half is recorded under the exception: no Qt install exists. **Owner:** none. |
| Upgrade with live agents | **Have:** today all 63 original native PID/births survived GUI Quit/restart; rollouts keep 63–64 agents. Both are Next→Next only. **Covered (coordinator, 2026-11-05):** the maintained owner-account Next→Next upgrades with live agents. On arch, linux-shell-u's owner session moved `7a60dceeb` → `8e74aa884` with the same Codex/hgs/tmux/job/pane, exact draft bytes and 57 native processes untouched (#1472). On the Mac, the queue rollouts moved `7ae0c2bf6` → `7a60dceeb` → `0f6017973` with native processes untouched and drafts kept. Existing Qt users' Qt→Next path is covered by the CI install/package checks and the updater suites (`test_tray_update`, `test_macos_switch_updater`); an owner-account Qt→Next run is waived with the Qt rollback. **Owner:** none. | **Have:** rollouts keep agents (Next→Next). **Covered (coordinator, 2026-11-05):** the maintained owner-account Next→Next upgrades with live agents. On arch, linux-shell-u's owner session moved `7a60dceeb` → `8e74aa884` with the same Codex/hgs/tmux/job/pane, exact draft bytes and 57 native processes untouched (#1472). On the Mac, the queue rollouts moved `7ae0c2bf6` → `7a60dceeb` → `0f6017973` with native processes untouched and drafts kept. Existing Qt users' Qt→Next path is covered by the CI install/package checks and the updater suites (`test_tray_update`, `test_macos_switch_updater`); an owner-account Qt→Next run is waived with the Qt rollback. **Owner:** none. | **Have:** legacy case, Qt ZIP→Next installer in its own folder, with seeded config, drafts and session files preserved; no agents. **Auto:** a private profile running a fixture-agent session and ConPTY job through the Qt ZIP's `hgs.exe`, upgraded to Next; verify the same session, run, conversation, job and draft. **Owner:** question 5. |
| One-time import | **Have:** 9 handled preference families unchanged (11-02); retained-source warning `.66.5.11`. **Blocked:** G3 source work `.66.5.2.1/.66.5.2/.66.5.4` (migrate-u). **Auto after G3:** an unattended PRIVATE-profile fixture, like va-kids importonce (coordinator, 2026-11-05). Seed Qt state: preferences, a Qt draft, a legacy per-computer project list and shared accounts. Start Next: the once-only notice, Got it, then two restarts with no notice, reseed or replay. Qt sources stay byte-identical; projects are imported, drafts are not; check the Migration report. linux-shell-u runs it after their owner segment. | **Blocked:** G3 as on arch. **Auto after G3:** the same fixture, run by macbundle-u after their owner segment. **Owner:** none. | **Have:** none. The owner account has no Qt registry or INI state (inventory 06:10Z). **Auto:** none on the main account; seeding a legacy source there would manufacture state. **Owner:** question 5. |
| Service registration / readiness | **Have:** readiness PASS (`5c110055`); today a background no-op, with presence, reads and delivery continuing while the GUI was absent. Custom-unit refusal still PENDING. **Auto:** custom-unit refusal against a private unit directory; one exact-target delivered read with the UI closed, on a synthetic session. **Owner:** none. | **Have:** today the service and host were ready, drain and revoke settled, and wrapper `.4.11` failed closed (macbundle-u fix). **Auto:** inspect the `zerus-setup` LaunchAgent; foreign-definition refusal against a private directory; UI-closed duties on a synthetic session. **Owner:** none. | **Have:** passive registration [x] (11-02); today install/status/remove, cold activation refusal and no service start. **Auto:** start the registered task in session 1; check the pipe identity and readiness with the UI closed; remove it afterwards. **Owner:** the real logon trigger needs a sign-out/sign-in (question 6). |
| Notifications / clicks | **Have:** today one Post Delivered, with its native ID in the history. The click FAILED to reopen the GUI (`.66.3.42`); banner appearance was not observed. **Auto after the `.42` fix:** one synthetic Post, then a uinput click on the banner (question 3), plus stale and withdrawn refusal. **Owner:** the click if question 3 is declined; lock/idle (question 6). | **Have:** a Post plus an owner click into the project (`f1833dec`; selection unqualified; drain FAILED). Today a Post was Delivered and drain settled; Withdraw is Unknown; no click. **Auto:** Post, drain and withdraw rerun after `.4.11`; selection readback (#1210). **Owner:** the click and selection (question 1); lock/idle (question 6). | **Have:** none. The GUI routes #1170/#1174/#1175 are merged. **Auto:** a service-owned Toast from a synthetic session, clicked through UI Automation, with stale and withdrawn refusal and a cold-GUI launch (win-notify-cl plan `.66.3.30.8`). **Owner:** lock/idle (question 6). |
| Tray / menu bar / lifecycle | **Have:** today the panel tooltip, menu and primary reopen; normal Quit and maintained unit start; drafts byte-identical. **Auto:** second-instance routing, background start, Quit with a pending draft. **Owner:** none. | **Have:** none. **Auto:** reopen and second instance through `open`; maintained Quit. **Owner:** menu-bar clicks, Services, Hide and Show All (question 1). | **Have:** legacy case first-start window, own tray menu (Open Zerus / No remote machines / Open in / Quit) and native Quit (fixture). **Auto:** on the main install, second instance, background start, draft-safe exit, and setting/removing the login opt-in, all through UI Automation. **Owner:** the real login start (question 6). |
| Terminal | **Have:** none. **Auto:** a synthetic session and a folder terminal on fixture paths with spaces and non-ASCII; exact argv from `/proc`; resize, paste and reattach after client exit. **Owner:** none. | **Have:** none. **Auto:** none; driving Terminal.app needs AX. **Owner:** the expected Apple Events consent and GUI driving (questions 1 and 2). | **Have:** none. **Auto:** a fixture session through Windows Terminal, PowerShell and ConPTY on paths with spaces and non-ASCII; exact selection through UI Automation; paste and resize through SendInput; continuity after GUI exit. **Owner:** none. |
| Voice / video | **Have:** today microphone record, Escape to preview, playback state and discard PASS. Camera FAILED (`.66.3.41`; fix #1338 merged, awaiting rollout). Audible playback and transcription unqualified. **Auto:** real-camera preview and cancel after the rollout; espeak-ng speech into a virtual source, then transcription with the existing speech profile; playback proven by sink-monitor capture. **Owner:** question 4. A new speech-provider credential, if no profile exists. | **Have:** none. **Auto:** none; consent state is unknown, and synthetic input needs a virtual audio driver. **Owner:** question 2. | **Have:** none. Voice for local Windows sessions is unsupported by design (installation docs). **Auto:** check through UI Automation that the refusal is visible. **Owner:** none. |
| UI / durable parity | **Have:** approvals OFF and generic-agent refusal (11-02); today 128 draft records byte-identical. **Auto:** a fixture walk of the installed build in a private Xvfb; on the owner account, read-only service queries plus a uinput-driven walk (question 3); one authenticated frontend request round trip. **Owner:** none if question 3 is approved. | **Have:** none. **Auto:** read-only service and catalog queries. **Owner:** the GUI walk (question 1). | **Have:** first-start window only. **Auto:** a UI Automation walk against a fixture profile with a synthetic session; one frontend request through a ready service to its durable result. **Owner:** none. |
| Qt rollback / reimport | Waived by owner 2026-11-05 on every platform; no Qt round trip. | Waived by owner 2026-11-05. | Waived by owner 2026-11-05; question 5 no longer covers Qt rollback. |
| Open failures | `.66.3.41`: rerun the real camera after the rollout (desktop-u). `.66.3.42`: rerun after macbundle-u's fix, with the click as above. | `.66.3.30.2.4`: click and navigation owner-pending (AX untrusted, loginwindow frontmost); wrapper `.4.11`. | `.66.8.1.11` (P2, installer refuses a non-owned folder), `.66.8.1.10` (CR CR LF), both win-accept-cl; `.66.8.1.12` (P4 log line). |

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
