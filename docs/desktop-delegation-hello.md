# Desktop instance identity challenge

The existing Next single-instance listener remains the owner of the canonical
`hgs-tray` socket. The service never binds, removes or brokers that socket.
`show` and `sessions`, the startup lock, updater discovery and rollback keep
their existing behavior. This additive protocol only identifies a compatible
live frontend. It grants no writer permit, delegation, drain proof or takeover.

Callers first require the exact trimmed help line
`Desktop owner ABI: desktop-delegation-v1` from their selected local CLI before
invoking `hgs desktop owner-status --json`. Older CLIs may interpret an unknown
command as a session launch. The new command makes one read-only socket probe;
it does not start the service, write state, invoke children or retry.

The query is the 59-byte ASCII line `desktop-delegation-v1 NONCE\n`, where
NONCE is a fresh canonical UUID minted for each probe. It fits the listener's
existing 64-byte line budget. Next replies with one JSON line, at most 4096
bytes including LF. Its closed schema carries `abi`, `version:1`, the exact
`nonce`, `application`, `gui`, `build` (version, source_commit, source_dirty),
`instance_epoch`, `writer_epoch`, `configuration`, `live` and `replay`.
`writer_epoch` is currently null; the hello cannot assert effect authority.
The app chooses an instance UUID at startup. This is not a writer epoch.

The accepted identity is the production `com.hgdev.hgs-tray` Tauri app, a known
clean source build, live and not replaying, with an exact configuration hash.
Configuration comes once from a background probe of the selected local hgs:
ordinary `--help` must advertise the exact `Desktop service ABI: desktop-v1`
line before `desktop status --json` is called. Each child has a five-second
owned-child deadline and bounded output; failed/absent capability stays unknown
for the instance. No per-socket-probe child, GUI-thread I/O or automatic retry
is introduced. A second launch that forwards activation never starts this
configuration probe. Replay and nonproduction builds do not probe a CLI.
The listener only reads cached identity. Changing configuration requires a new
instance; later handoff must independently revalidate the selected configuration.

Unix peers authenticate with SO_PEERCRED or getpeereid. The probe uses
nonblocking connect and one total one-second socket deadline, not a timeout
renewed by each byte. A full backlog or a connect that cannot complete within the deadline stays unknown. The
existing Qt path selection is shared with live Next, preserving lexical cleanup
and macOS's per-user temporary directory without expanding `/tmp` symlinks.
Only a missing endpoint is `absent`. A stale, unsafe or unreachable existing
entry is `unknown`; a connected peer ignoring/rejecting the query or returning
an unsolicited, stale, mismatched, newer, dev, replay, malformed, late or
oversized reply is `live_legacy_or_unknown`. These are passive observations,
not service error states. They never establish that no frontend exists.
A valid reply is `live_next`, still with authority, takeover and writers false.
Future effect batches must apply their own bounded scheduling and fresh checks;
this command has no polling loop or retained cache.

Windows currently reports unsupported/unknown. Its permanent instance lock and
native ownership protocol need their separate runtime qualification. No Unix
probe result substitutes for that proof.
