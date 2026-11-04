# Mac original helper observation

`desktop::relay::Connection::observe_original_helper` observes the caller's
same owned, unreaped `Child` before private stdin is taken or written. Only the
connection's retained genuine own-bundle `Selection` supplies the helper path,
requirement and native CDHash. An ordinary unpaired route cannot supply them.

The observation checks the original birth, direct parent and user, obtains a
kernel task-name right for that child, then reads `TASK_AUDIT_TOKEN` with the
exact native count. Security receives only `kSecGuestAttributeAudit`, never a
PID guest attribute. Fresh live code checks compare the selected requirement,
native CDHash and executable; repeated token/birth checks and full bundle
generation checks refuse changes. The task-name right is released on every
return. No task control right or process suspension is used.

On refusal, stdin is closed. The caller still owns the same child, output pipes
and worker/admission charges, and must kill and reap that child through its
existing cleanup path, including past an outer timeout. Call the primitive
before any write; never put private data in argv or the environment. Security
runs synchronously off UI and actor threads.

The held task right refuses an exec that happened before a token re-read. The
remaining window is after the final re-read, when the verified helper's own code
can still exec. A successful observation is point-in-time evidence, not an exec
fence, and does not grant an effect, writer or native permission.

The primitive is separate from the service-to-app host check and from
connected-peer observations. Tests use isolated Apple-signed system helpers to
exercise real task tokens and live code, including a same-child exec before
input release. These are not a genuine installed Zerus pair positive or owner
acceptance. The service-native termination caller has its own narrow
`Image::selected` adapter: it retains the original `Connection`, refuses
unpaired/path-only routes and invokes this observation for every spawned helper.
It then rechecks the same Service/configuration/epoch/generation/relay and
cancellation immediately before handing the child to a runner; retention RPCs
repeat that check after original-child registration. Rejection closes input and
stays latched, with the same child and actual resources retained for cleanup.
See [the caller boundary](desktop-termination-outcomes.md#execution-custody-and-platform-boundary).
Its ordinary/G1 source review and native caller controls are independent of
this primitive's evidence. The installed-pair Terminate/Remove positive belongs
to the next coordinated Mac window; it is required for actual caller
qualification and cannot be inferred from a component fixture.

The native interfaces follow Apple's [task-info definitions](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/task_info.h)
and [task-name info conversion](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c).
