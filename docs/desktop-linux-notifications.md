# Linux notification method port

`client/core/src/linux_notify.rs` contains pure saved-ID compatibility policy.
Its default build has no transport dependencies or OS calls. The explicit
`linux-native-notify` feature adds `linux_notify::transport` on Linux only,
using the same zbus 5.19, async-io 2.6, futures-lite 2.6 and Tokio 1 versions
already used by the Tauri client. Tokio supplies only a runtime-context probe;
this port never creates a runtime. The root CLI does not enable this feature.

The method port has no production caller. The existing Tauri poster retains its
filesystem and Qt readers, Saved pending barrier, persistence and OS authority.
This extraction neither hands off that writer nor admits service execution.
In particular, its strict post-call Unknown classification does not replace the
GUI adapter's existing known-refusal behavior.

## API and ownership

`Transport::connect` takes an explicit bus address, budget and cancellation flag.
`namespace` reads the current authenticated connection's GetId and the daemon's
unique owner. `post` takes that expected namespace, title and body; `withdraw`
takes a `NativeId` containing the namespace and positive `NonZeroU32` ID.
`shutdown` returns `Pending` or `Drained` while leaving the transport owned.
All methods require exclusive access; no cloneable connection escapes.

`Namespace` and `NativeId` are data only. G1 must supply current authenticated
producer drain, writer/service epochs, adapter lease, fresh reservation and the
exact native job before any call. G1 binds a result to that original ticket and
owns its indexed bus/owner/ID/token/revision mapping and durable activation
tombstones. A withdrawal requires tombstoning first, then the OS close. Neither
imported state nor a method result establishes those permissions.

## Bounds and outcomes

Each budget must be positive and at most five seconds. A method's absolute
deadline includes its fresh GetId/GetNameOwner preflight and native call.
Cancellation is checked before admission to the call and while driving I/O,
with a ten-millisecond polling interval. Title and body are each at most 16 KiB
of UTF-8. Payload text escaping, app name, icon, actions, hints and timeout match
the existing GUI adapter.

The native destination is always the freshly checked unique owner, never the
well-known name after checking it. Bus lookup replies must have the bus sender;
successful native replies must have the exact daemon sender. A missing daemon,
namespace mismatch or failed lookup refuses before the native call. Every
possibly-issued error, including a daemon MethodError, malformed/zero identity,
timeout, cancellation or disconnect, is `Unknown`. No automatic retry exists.
Outcome enums contain allowlisted metadata, never private daemon error bodies.

The connection's zbus executor thread is disabled. The caller's own thread drives
the executor, with one queued message and no added worker, callback or result
queue. zbus still enforces its protocol frame limit of 128 MiB; the 16 KiB text
limit is a request limit, not a claim of a smaller raw incoming frame limit.
This port does not install a signal stream or dispatch activation.

The port refuses construction and methods inside an active Tokio context, and
keeps shutdown pending there. This also covers dependency-feature unification:
zbus can otherwise choose Tokio, whose executor `is_empty` does not account for
its tasks. Connections are created and driven outside Tokio, so actual async-io
task accounting remains available. Tests cover both zbus backends being enabled.

## Actual resource drain

Shutdown first disables calls. `Pending` retains the port and execution charge;
the caller may continue bounded shutdown attempts. A successful close or expired
deadline alone is not completion. `Drained` requires the connection to be dropped
and its owned executor to have no unfinished connection/reader tasks. Dropping a
port is not an actor-authorized release receipt.

The later G1 adapter must also account for its signal stream, callbacks and
queued results before releasing admission. Those resources, service revoke and
restart/reboot custody, durable native receipts and positive IPC are not supplied
by this method-only slice. The existing single-instance/SNI/menu routes and
Exit/`exit_work` versus updater `quit_with_flush` boundary stay unchanged.
