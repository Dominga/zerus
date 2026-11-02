# Notification preferences and foreground observations

The closed wire foundation is `client/core/src/desktop/notification_control.rs`.
Its ABI is `desktop-notification-controls-v1`. This source does not expose a
callable CLI route or advertise its help marker. Root admission and the native
client companion follow the stage 2 host attachment. Consumers must check the
actual marker before dispatch, independently of DTO availability.

## Preference transfer

`Command::Configure` carries the logical request UUID, original preference
journal/revision, and exactly `Preferences { optional_questions }`. The default
is false. There is no notification-enable field, mode setting, approval opt-in
or foreground value. An authenticated service-mode frontend initializes the
unconfigured record from the existing optional-question preference; subsequent
edits use the last verified service journal/revision. Reconnect reads the
retained value and must not silently overwrite a configured record with a stale
UI setting. Missing or uncertain status is unconfigured/unknown, not permission
to replace retained settings.

The intended sender is the existing native frontend original-child runner,
using the paired service relay, retained Responder registration and resources.
Root adds a closed `NotificationControl { control: Command }` frontend request
arm, binds it to cached native Service mode, and rechecks its opaque Validated
permit after the transaction starts. Configure uses the serial operation key
`notification_preferences`. It remains safe-passive when the authenticated
route and store are writable; it grants neither ownership nor writers.

Status has an exact configuration/service-epoch Scope, independent preference
journal, revision, configured bit and preference. Saved receipts bind the
original UUID, configuration, fingerprint, journal, resulting revision and
exact value. A historical receipt acknowledges the edit, not currentness.
Journal/revision mismatch refuses before replacement. No source settings file,
readiness value, caption or watch payload supplies admission.

Unknown edits retain their logical UUID and original child/resource accounting.
Only read-only Query/Lookup of that original Configure recovers a result.
Missing receipts, replaced journals and an initially absent journal remain
unknown. A Root-proven not-applied result is possible only in the unchanged
original nonempty journal/revision; a new logical edit also requires confirmed
original-child exit. Unknown edits are never resent. Receipts and the singleton
must be bounded; schema allocation belongs to the implementation, not this wire
foundation.

## Native foreground snapshots

`Command::Focus` contains only a fresh request UUID and uses the independent
serial operation key `notification_focus`. Its native original-child evidence
contains `NativeFocus { connection_id, revision, state }`: the current retained
service connection incarnation and native window state Unknown, Background or
Foreground. The native Responder cache starts Unknown; Rust window focus events
update it. Disconnect/reconnect invalidates the cache/connection incarnation;
the initial state is never inferred from a UI file, a JSON focused flag,
ActivationWait, source activity or service readiness. Native mode cannot
register this service-mode request. Ordinary Hello/evidence stays compatible;
the additional native observation is bound only to this request arm.

The client sends one bounded off-UI snapshot at a time, at most every two
seconds while connected, and promptly on a native focus transition. It retains
the same paired relay connection selection and original child/resources through
each exchange. A pending exchange coalesces later native changes; no unbounded
worker or input queue is created. Focus is suppression data, not a permission.

Root retains at most one original canonical-frontend proof/connection lease.
A verified Background/Unknown update, old connection, failed proof, known UI
exit or verified disconnect clears suppression immediately. An uncertain lost
connection or reply cannot renew it: the last Foreground snapshot expires on a
monotonic deadline within five seconds. The actor checks cheap held-process
liveness every turn, bounded full proof periodically, and full proof before
accepting a replacement lease. The original mutation deadline is not extended.
A stale native revision or a former connection cannot replace a newer lease.

FocusReply carries the exact UUID and Scope, native observation and remaining
TTL (zero except for Foreground). It has no durable receipt, lookup, journal,
replay, wall-clock expiry or persisted identity. A lost reply is followed only
by a fresh native observation with a new UUID after original custody drains.
Server restart starts with no focus lease; reconnect starts native Unknown and
requires a fresh observation. The attention runtime consumes this bounded lease
to suppress newly observed foreground edges without replaying them after blur.

Required notifications still depend on the independent durable ownership
grant and actual original host. Preferences and focus cannot rearm a revoked
grant, start a service, authorize approvals or establish closed-UI launch mode.
