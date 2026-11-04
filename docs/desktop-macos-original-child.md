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
and worker/admission charges through its existing stop/reap/cleanup path,
including past an outer timeout. Security runs synchronously off UI and actor
threads. A successful observation is point-in-time evidence: it cannot prevent
a later exec and does not grant an effect, writer or native permission.

The primitive is separate from the service-to-app host check and from
connected-peer observations. Tests use isolated Apple-signed system helpers to
exercise real task tokens and live code, including a same-child exec before
input release. These are not a genuine installed Zerus pair positive or owner
acceptance. The new service-native termination caller remains unavailable on
Mac until its native adversarial evidence and ordinary/G1 source reviews are
complete; this primitive does not enable that caller.

The native interfaces follow Apple's [task-info definitions](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/task_info.h)
and [task-name info conversion](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/task.c).
