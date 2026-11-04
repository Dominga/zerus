# Native Service consumer activation

The orchestration rollout keeps the startup default unchanged. Select the
consumer with `ZERUS_CLIENT_DESKTOP_SERVICE=1`; the later Linux/macOS default
change is a separate release step after this path reaches both owner machines.
Windows keeps its explicit opt-in and maintained manual task transition.

The paired backend is prepared before the GUI starts, through the existing
installer/updater. The consumer selects that same backend, reads its native
configuration and registration, and obtains a fresh authenticated Service Grant.
Only a newly installed, implicitly passive registration has `mode_selection`
`default`. At this initial connection the native controller may select background
through the maintained helper. A running background registration is a no-op.
Ordinary updates preserve mode. Older records without provenance are `retained`;
an explicit passive selection or rollback remains paused until the owner chooses
**Resume background work**.

A revoked or rearm-required installation opens the limited consumer. Resume is
one explicit action: authenticated Rearm when needed, followed by the same
helper activation. If the current Service Grant is already valid, Resume only
selects background. The helper independently checks the original installed
frontend or foreground terminal, current Grant, selected CLI, configuration and
owned registration; local status and JSON are routing observations only.
Automatic read-only reconnection never grants, rearms, imports or activates.
There is no legacy GUI writer fallback, and automatic approvals stay off.

## Original activation and failure handling

The native controller reserves one activation UUID before spawning the original
helper. The helper durably records that UUID and its original Grant/image/
definition binding before any mode or manager change. A known completion can be
written only by that helper's retained process-local attempt. The controller
keeps the original child and both pipes charged through actual exit and EOF.
The same immutable record survives GUI/helper restart; an uncertain attempt is
looked up by its original UUID and is never offered again or upgraded from a
current readiness observation. The fixed journal retains at most 64 entries
within the existing 64 KiB registration-record bound. Byte admission may refuse
earlier: every admitted intent reserves room for its largest later review and
scoped completion. Exhaustion preserves all originals and the current mode;
status and original-ID lookup remain available. There is no journal reset or
identity eviction route.

- A helper refusal leaves the consumer usable. **Check result** looks up the
  original. A verified missing/not-started result offers Resume again; a missing
  helper or obsolete helper requires a maintained CLI/GUI update. Never clear a
  retained revoke or alter the registration by hand.
- A Grant timeout keeps its original ownership intent. **Check result** queries
  that original; activation waits for a confirmed current Grant. Revoked access
  offers explicit Resume, rather than silently rearming.
- A service crash after activation uses bounded read-only reconnect and keeps
  the last known catalog visible. **Retry now** checks the connection. It does
  not repeat the activation command; an uncertain activation stays on Check.

## Explicit review of an interrupted Unix activation

An interrupted activation keeps its original UUID, binding and **Unknown**
manager result. Reviewing it changes separate metadata only. It neither stops
nor starts a service, changes a Grant/revoke, joins or releases an original
resource charge, nor establishes that the previous manager effect succeeded.
Status, Retry and automatic reconnect never perform this review.

Use the updated maintained helper from a foreground terminal outside Zerus
agents. Select the same CLI, configuration and state as the registration, and
replace `UUID` with the original `activation_pending` UUID from helper status:

```sh
# Linux; adjust the installed prefix to match the selected CLI.
python3 "$HOME/.local/lib/zerus/install-linux-desktop-service.py" \
  review-activation --hgs "$HOME/.local/bin/hgs" --activation-id UUID --json

# macOS; APP is the selected installed Zerus app.
python3 "$APP/Contents/Resources/setup/install-macos-desktop-service.py" \
  "$APP/Contents/MacOS/hgs" --review-activation --activation-id UUID --json
```

Pass the original `--config-dir` and `--state-dir` when they are nondefault.
Do not select a mode in this command. The helper must acquire the same exclusive
registration lock that the original helper held through its effects and child
cleanup. A busy lock refuses; Review never kills that helper or its consumer.

New attempts capture the admitted native consumer's PID/birth, native machine
identity and kernel boot identity. Only a different native boot on that same
machine proves that all the original processes are gone. This source slice
does not expose a same-boot acknowledgement, even when the consumer has exited
and the helper lock has released: an abruptly killed helper might have left an
orphaned manager child or unconfirmed pipe cleanup.

Legacy records have no trustworthy process or boot ordering. Their first
explicit Review durably records a reboot barrier bound to the exact original
UUID/binding, machine and current kernel boot. It returns `reboot_required` and
does **not** acknowledge the attempt. **Restart the computer, then use Review
interrupted mode change again** (or repeat the exact helper command above).
The same reboot-barrier route applies to a new attempt from the current boot.
Only a genuinely different kernel boot on the same machine permits the
`reviewed_unknown` disposition. Same-boot, unavailable or inconsistent identity
evidence refuses. File dates and wall-clock age never substitute for the barrier.

The original result remains Unknown permanently, including after review and
restart. Automatic/default activation remains suppressed. Any later mode choice
is a separate explicit confirmation with a new request and complete fresh
native caller, current Grant, registration/manager and endpoint preflight.
Acknowledgement supplies no permission to replay or adopt the old operation.
In the Service consumer, open **Settings > About** and use **Review interrupted
mode change**. It displays the original UUID and configuration and asks for a
native confirmation. The first confirmation changes metadata only. If a reboot
barrier is needed, use **Restart**, then explicitly Review again. Shutdown with
Fast Startup may retain the same kernel boot and does not qualify.

After `reviewed_unknown`, Settings offers a separately confirmed background
choice (and a passive choice on Windows). Each choice retains one NEW request
UUID plus the exact reviewed UUID. A lost reply queries only that new original;
it never substitutes another request. Changed selection, configuration, helper
or original during either confirmation refuses. Generic Resume, Check result,
Retry, reconnect and GUI restart cannot confirm either action. Older maintained
helpers need an update before they can publish the scoped choice receipt.

The permanent default/manager latch stays set. Ordinary Service work becomes
eligible only after the new choice's native original-ID result is durably
settled to background and is still the latest applicable choice. Its retained
linkage must match the reviewed UUID, configuration, actual current service
epoch/generation and current ownership journal/epoch. A fresh ownership read
must also match this consumer's own native Grant receipt, with current identity
and no revoke or rearm requirement. This history never supplies per-kind import,
cleanup or manager authority. Existing source capture, native admission and
original child/pipe/Owner custody still apply. Only that second confirmed
continuation can arm existing imports or refresh the SAME-origin Grant while
keeping its epoch. Status, Retry, backoff and read-only reconnect do not queue
those operations. Passive, unknown, changed-origin or changed-epoch evidence
closes eligibility; missing authority needs its explicit action again.

Component controls model the confirmation response and manager/native caller
outcomes. The actual OS dialogs, installed Settings path, Windows Fast Startup
versus Restart BootId behavior and native PC qualification remain outstanding.

## Qt rollback

Use the existing updater/installer rollback. Before a current desktop service
can be replaced by Qt, the updater asks the maintained helper to select passive.
The helper's read-only status must confirm an existing managed registration;
the CLI's lifecycle ABI alone does not establish one. An unregistered host keeps
the plain revoke-only Qt path, without creating or enabling a desktop service.
That helper retains its original revoke UUID and original EOF hold across the
service transition and verified passive readiness. A refusal keeps Qt stopped.
The updater then reserves its own original final revoke UUID durably before
offering the authority reduction. An interrupted updater only reads that same
UUID; another invocation does not generate a replacement command or rerun the
mode helper while that result is unknown. The current-scope Root receipt must
confirm revocation and drain before Qt can proceed. An unknown result keeps the
current GUI/rollback copy available and prints the exact original request for
review. No raw manager stop, native-agent signal or replay is introduced.
The final-revoke journal lives beside the draft-snapshot directory. An interrupted
original at the older snapshot location stays on that same lookup-only path;
finding both locations requires review before any new offer.

Cutover and rollback preserve accounts, catalogs, drafts, native agents, tmux
servers and DeepSeek hosts. Component tests use private manager/transport
fixtures and original child objects. The real Linux helper's mode-transition
test checks its written unit's `KillMode=process` and exact manager-call scope;
this does not qualify installed agent survival. Installed Linux/macOS delivery,
activation, presence and device acceptance remain separate release evidence.
