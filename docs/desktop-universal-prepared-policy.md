# Universal prepared replacement policy

**Status: PARKED, 2026-11-04.** No production producer prepare/controller is
planned since the owner direction of 2026-11-01 recorded in `zerus-5wmh.66.3.23`
(`hgs desktop` owns background duties and Zerus Next is UI only; no
universal-old-binary proof gate). `zerus-5wmh.66.3.23.18.2` was closed as
superseded, not as qualified. This contract is kept for a reactivation: any
future production producer prepare/controller reopens that bead and must satisfy
both conditions below first. Behavior is unchanged: all production prepare
remains fenced. The startup, presence and revoke guards stay active; only their
prepared-row branch sees no production-created row, and it still refuses one.
Existing per-kind and current-ownership safeguards are unaffected.

Status: contract and source inventory only, 2026-11-01. Tracks
`zerus-5wmh.66.3.23.18.2.2` under the Qt retirement epic. This document changes
no startup, installer, rollback, service, writer or protocol behavior. All
production producer preparation remains fenced.

## Three different facts

| Fact | Existing evidence | What it does not establish |
| --- | --- | --- |
| Legacy-compatible startup | A readable selected CLI lacks the startup marker; Next preserves its daily GUI, writers and asynchronous cached configuration. | Held exclusion, current producer identity, prepared-state consumption or a future replacement barrier. |
| Verified original helper completion | The supported startup helper held its original exclusion through GUI writer registration, then released; the parent verified the exact exchange, child exit, pipe destruction and original worker join. | A role drain, completed reconciliation, continuing writer ownership or coverage of a later startup through another artifact. |
| Universal future-replacement enforcement | Every maintained startup and rollback consumes prepared state independently of the selected daily-operations CLI, together with fail-safe service handling of outside legacy writers. | This is a required future contract, not an implemented or qualified capability. |

The first two paths currently converge on `Responder::configure_once` in
[desktop_hello.rs](../client/src-tauri/src/desktop_hello.rs). Both can publish the
same configuration digest. The ordinary [Responder](../client/core/src/desktop/mod.rs)
hello therefore does not identify their startup origin. The
[Bootstrap](../client/core/src/run/startup.rs) is stronger evidence only for its
own original completed exchange. A cached hello, configuration, help marker,
source version, successful probe or per-instance origin tag cannot substitute
for the third fact.

In particular, tagging today's held instance would not stop a later canonical
replacement from taking the readable-old-CLI compatibility path after the
original process exits or crashes. The
[existing conditional consumer](desktop-startup-consumer.md) remains conditional.
[Recording recovered abort intent](desktop-producer-prepared.md) also leaves the
original prepared record fenced; it supplies no completed-reconciliation proof.

## Compatibility that remains required

Current Next with a readable older selected CLI keeps the daily GUI, existing
writers and read-only cached identity, without counting any of them as
currentness. This policy does not remove that compatibility path or turn its
ordinary startup into a new error or permission dialog.

The maintained rollback copy retained by
[update-installed.py](../tray/update-installed.py) also remains supported. It is
Qt during this transition. Preserve its current draft/settings retention,
verified runnable-artifact checks, numbered-build guard and native-process
protections. G5/G6 retire that Qt rollback as part of Qt retirement; this contract
does not delete, replace or relabel it in advance.

## Necessary conditions before production prepare

Both conditions below are required. They are additional prerequisites, not a
replacement for current authenticated origin, complete role/resource barriers,
profile qualification, durable reconciliation or G3 publication requirements.

1. **Every maintained startup and rollback consumes prepared state in the GUI
   bootstrap independently of the selected CLI.** It must do so before its GUI
   writers can start, including service-offline, crash, restart, rollback and
   explicit older-CLI selection. This requires moving the kept rollback copy
   forward to a G1-aware Next build and retiring the Qt rollback through G5/G6.
   A path missing that consumer remains an open gate for production prepare.
2. **The service fails safe for a legacy GUI writer started outside maintained
   paths.** Manually launched old binaries are outside supported startup, but
   they must not create double writers. The service needs qualified negative
   coverage and must stand down or pin when such a writer is present or its
   exclusion is unknown. The detector and its continuous enforcement are not
   implemented or qualified here.

A service cannot infer that condition 2 holds because a canonical socket is
absent, a PID disappeared, a process name matches, a probe timed out or a cached
observation looked quiet. Its future proof must cover actual writer identity,
lifetime and races with admission. Unknown coverage never becomes known zero.
If interference occurs after an attempt started, admissions must be fenced and
unconfirmed work retained. Stand-down requires its actual resource completion;
it does not erase durable intent or manufacture an Idle/Drained receipt. No
existing gate reopens automatically. Stand down only the service's own roles and
resources; do not stop a foreign GUI, native agents, tmux or unrelated services
to force a quiet result.

The existing handoff/presence locks and supported CLI checks remain their exact
reviewed mechanisms. They do not establish coverage of every legacy GUI writer
or arbitrary older CLI. This document introduces no sentinel, persistent index,
owner-store inspection, socket brokering, current permit or new guard API.

## Maintained startup and rollback inventory

This is a source inventory, not an installation audit or execution proof.
Packaging definitions are included regardless of their publication status.
No installed application, service definition or owner state was inspected.

| Path | Current source and behavior | Remaining enforcement requirement |
| --- | --- | --- |
| Linux Next source install | [tray/install.sh](../tray/install.sh) accepts `HGS_LINUX_GUI=client` only and uses the maintained updater. [The user unit](../tray/packaging/hgs-tray.service) starts the installed GUI; [the desktop launcher](../tray/packaging/hgs-tray.desktop) requests Sessions. | Both cold login/service starts and a launcher that must claim a new instance need the CLI-independent consumer. Existing forwarding is not a new writer or universal evidence. |
| Linux system package and compatibility links | [The packaged unit](../packaging/linux/hgs-tray.service), [launcher](../packaging/linux/hgs-tray.desktop) and explicit [zerus-setup](../packaging/linux/zerus-setup) name the installed GUI or supply compatibility links. | Package ownership, a service-manager label or a compatibility link does not prove bootstrap consumption or legacy writer exclusion. |
| Mac source install and remote entry | [tray/install.sh](../tray/install.sh) accepts `HGS_MAC_GUI=client` only and generates the tray LaunchAgent. [mac-install.sh](../mac-install.sh) is the maintained remote source-install entry. | The selected GUI and its actual CLI/configuration origin require independent qualification. A source install or current CLI version does not attest every later launch. |
| Mac LaunchAgent, manual app open and packaged setup | [The LaunchAgent template](../tray/packaging/com.hgdev.hgs-tray.plist) starts the app executable with an explicit `--hgs`; [packaged zerus-setup](../packaging/macos/zerus-setup) substitutes the actual app/CLI paths. Finder/manual opens still enter that GUI's bootstrap. | Login, crash restart, explicit app open and later changed definitions must all be covered. Bare launchd PATH, an argument, bundle identity or signature alone is not the negative consumer. |
| Current canonical Next, Linux/macOS | [lib.rs](../client/src-tauri/src/lib.rs) claims the canonical instance before [desktop_hello::hold](../client/src-tauri/src/desktop_hello.rs), then calls finish after setup. Replay/other identities stay outside this production bootstrap. | Supported-helper completion and readable-old-CLI compatibility remain distinct from universal replacement. A different/replay identity never establishes production coverage. |
| Windows portable, shortcut and installer relaunch definitions | [windows_portable.py](../scripts/windows_portable.py) validates the `hgs.exe`/Qt `hgs-tray.exe` layout. [windows_installer.py](../scripts/windows_installer.py) describes its unchanged payload; [zerus.iss](../packaging/windows/zerus.iss) defines shortcut, post-install and relaunch entries. | Current Next's conditional bootstrap is Linux/macOS only. No Windows startup, rollback or native-host capability is qualified as this prepared consumer. Installer availability is not asserted here. |
| Updater success, restoration and explicit rollback | [update-installed.py](../tray/update-installed.py) verifies the replacement and retains one rollback slot; a replaced verified Next build takes `desktop-previous`, while each displaced Qt image gets its own `desktop-qt-legacy/qt-*` entry for manual recovery; archive failures retain the named image and do not skip post-update checks. Failed replacement can restore the previous GUI. | Every artifact that can be restored or selected by the maintained updater must meet condition 1 before prepare. Successful update, draft preservation or a verified executable is insufficient. Retaining a Qt image is not qualification to run it. |
| Retired explicit Qt selection | The updater refuses Qt installation and Qt `--rollback`; the retained image is manual recovery only. | Failed-upgrade recovery and direct old-binary launches still need the original exclusion/receipt protections; source retirement grants no authority. |
| Direct legacy Qt or an old copied GUI outside maintained paths | [Qt main.cpp](../tray/src/main.cpp) performs its existing single-instance startup. Its normal GUI lifecycle and [DesktopPresence](../tray/src/DesktopPresence.cpp) remain unchanged. | Manual old launches are unsupported for the future handoff and require service-side fail-safe coverage under condition 2. Neither removing an installed copy nor checking a filename proves their absence. |

The inventory must be refreshed against the exact candidate artifacts before
qualification. In particular, a restore path is not removed from scope because
its artifact is older, unpaired, inactive, hidden or normally reached only after
an update failure. A link, command override or fallback must not silently select
a different unqualified artifact.

## Concrete limits of the existing Unix activation boundary

The existing Qt activation protocol is not a lifetime writer lease. The source
paths below explain why a guardian that merely occupies today's canonical
socket, holds the start lock or observes the GUI service cannot establish
condition 2. This is a source-derived counterexample to those proposed proofs,
not an executed concurrent-writer test or a new startup restriction.

| Boundary | Actual source behavior | Limit on future exclusion claims |
| --- | --- | --- |
| Relative activation name | Unix [SingleInstance::key](../tray/src/SingleInstance.h) returns `hgs-tray`. Qt's [server](https://github.com/qt/qtbase/blob/v6.11.0/src/network/socket/qlocalserver_unix.cpp) and [client](https://github.com/qt/qtbase/blob/v6.11.0/src/network/socket/qlocalsocket_unix.cpp) resolve a relative name through `QDir::tempPath()`. [Next's compatibility paths](../client/src-tauri/src/instance.rs) likewise separate the temporary socket path from the runtime start-lock path. | A socket in one temporary directory does not cover a different temporary namespace. The cited Qt version is reference source, not an audit of an installed Qt library. |
| Start serialization | [SingleInstance::start](../tray/src/SingleInstance.cpp) creates its `QLockFile` inside the startup loop. The default lock wait is 10 seconds; failure reports a diagnostic and continues with forwarding and socket claim. The lock is also released while waiting for the service. | Holding this lock is neither a permanent refusal nor a lifetime exclusion of GUI writers. A successfully claimed GUI does not retain this local lock through its writer lifetime. |
| Service wait | When forwarding does not connect, the same function waits for another reported service only until the default 30-second startup threshold, then attempts its own socket claim. | A service-manager observation is not an enduring writer barrier. A guardian cannot treat that bounded wait as refusal or resource completion. These thresholds are not whole-bootstrap time bounds. |
| Settings identity | [main.cpp](../tray/src/main.cpp) uses the fixed `hgdev` / `hgs-tray` application identity; [SettingsJson](../tray/src/SettingsJson.cpp) reads and writes default `QSettings`. [AppConfig::configPath](../tray/src/AppConfig.cpp) separately selects the home-based `tray.conf`. | Changing only `TMPDIR` does not establish a different settings/profile identity. Qt's [settings locations](https://doc.qt.io/qt-6/qsettings.html#platform-specific-notes) depend on platform, home/configuration and application identity, rather than the activation socket. |
| Writer bootstrap | After a successful claim and the normal tray-availability check, `main.cpp` preloads settings and constructs [TrayAgent](../tray/src/TrayAgent.cpp). Its attention observation path can store `attention/observed`; [read marks](../tray/src/ReadMarks.h) use the same settings abstraction. | Reaching an independent activation endpoint is not evidence that the resulting writer uses an independent durable store. A claim or a successful forward supplies no G1 role authority. |

For a concrete Linux source case, take otherwise identical startup inputs:
same user, HOME, configuration/runtime overrides, application identity and
selected CLI, but a different existing temporary directory for a manual old Qt
launch. The relative socket resolution
then differs while the settings inputs stay the same. Even a common start-lock
location does not repair this gap: its ordinary function lifetime and bounded
failure branch are not writer exclusion. If a service is reported but forwarding
still cannot connect, the existing service-wait fallback also remains. This
shows a possible path into the ordinary writer bootstrap; it does not assert
that a particular installed artifact launched, reached a native effect or
actually overlapped another writer.

The existing [Qt startup tests](../tray/tests/test_appconfig.cpp), including
`aStuckStartDelaysOthersBoundedly` and
`launcherClaimsWhenTheServiceStopsOrNeverListens`, encode the lock-failure and service-wait compatibility behavior with private paths. Reading
those tests is not a fresh execution result. No Qt, native service or concurrent
writer was launched for this inventory. Windows has a different lifetime-lock
path and is not covered by this Unix example; no Windows exclusion is inferred.

Keep the current compatibility behavior and maintained Qt rollback intact.
Do not turn these observations into a store scan, a search over temporary
directories, a process-name detector, a socket broker or a new guardian API.
Any future guardian design still needs both necessary conditions above, with
an exact artifact/configuration/lifetime argument and fresh private proof.
Unknown coverage keeps production prepare fenced.

## Trusted-guardian alternative and paired Mac origin

A maintained trusted guardian independent of the selected daily-operations CLI
is a direction to evaluate, not an implemented workaround. To satisfy condition
1 it would have to be reached by **every** maintained GUI bootstrap and rollback,
with verified artifact/configuration custody and fail-closed behavior across
absence, replacement, rollback, restart and partial update. Merely finding a
newer CLI, reading its help or attaching a guardian to the current Next instance
cannot give an older rollback that missing consumer. The guardian also needs
condition 2; it does not exclude manual legacy writers by existing.

The [paired Mac design](superpowers/specs/2026-10-31-local-macos-paired-bundle.md)
tracks a sealed app/CLI pair, independently installed CLI custody for production
self-spawns, explicit overrides and old Qt/Tauri rollback compatibility. Those
packaging facts must be assessed with both conditions. A valid pair, same-team
signature, selected sibling, exact source/hash, successful self-signed mechanics
test or installed LaunchAgent does not qualify a current producer or the
universal consumer. Genuine paired-origin/running-self qualification, installer
integration and current role/profile permissions remain separate gates.

No guardian command, sentinel, schema field, store read or service-side detector
is specified as implemented by this document. The next implementation needs its
own exact source/lifetime/read-set design and pinned review. Existing selected
CLI behavior and explicit choices remain compatible until that separate change
is agreed and qualified.

## Required later evidence

Qualification must bind the exact maintained artifacts and configuration, not a
list of expected names. Cover each inventory row, the kept rollback artifact,
failed-update restoration, supported helper and readable old CLI, service
absence/restart, partial progress and writer interference before/during/after
preparation. Include manually started old-writer controls for condition 2 and
unknown/delayed resource completion. Neither a process exit nor updater success
alone completes those controls.

Use private fixtures with fresh child environments and owned resources; never
exercise the owner's GUI, services, agents or mobile state. Retain raw hashed
logs and actual executed counts. Cross compilation and source inventories are
not native execution or coverage proof. Any unqualified inventory row or unknown
legacy-writer exclusion keeps **all production prepare fenced**. This does not
hold back independent dormant lifecycle prerequisites or their pinned reviews.
