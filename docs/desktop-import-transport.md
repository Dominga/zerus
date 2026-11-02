# Desktop import transport v1

G1 owns the private immutable evidence publisher. G3 owns source decoding and
`client/core::migration::Bundle`. This transport connects them through a selected
local `hgs` child and inherited stdin/stdout. It opens no listener, registers no
service and reads no Qt or interim source store itself.

This initial transport accepts **Staged evidence only**. It cannot write a
handoff marker, claim drain, acquire service ownership, consent to approvals or
enable any effect writer. Fresh runtime drain and takeover remain separate G1
work. A staged record must never be relabelled Drained because it once existed.

## Capability and process admission

Before spawning the publisher, run the selected local CLI's ordinary `--help`
and require the exact marker `Desktop import ABI: desktop-import-v1`. Bound
that probe to five seconds and a small captured output. If unsupported, leave
source stores intact and report unsupported; never call an unknown command
speculatively or fall back to another source decoder.

Spawn exactly `hgs desktop import-evidence --json`, using the same selected
`HGS_CONFIG_DIR` / `HGS_STATE_DIR` environment as the desktop service. Retain the
canonical configuration from supported read-only `hgs desktop status --json`;
never approximate its fingerprint. Generate and retain one canonical import UUID.
There is no shell expansion or caller-supplied destination path.

The child allows ten seconds without the next input chunk and at most 120
seconds for input, including streaming and staging I/O. The parent must enforce
a 130-second deadline for the entire child, including final filesystem sync,
and bound stdout/stderr capture (16 KiB each is sufficient). Cancel only this
owned publisher child on timeout; never a service, process group or agent.
A blocked input reader belongs only to that child and ends with its process.
Close the child's stdin after the final record so it can verify EOF. Timeouts
are unknown publication outcomes and never authorize automatic UUID reuse.

## Framing

Every header is one complete UTF-8 JSON line including its LF, at most 4,096
bytes, with no unknown fields. The first header is:

```json
{"abi":"desktop-import-v1","configuration":"64-lowercase-hex","source":"com.hgdev.hgs-tray","replay":false,"import_id":"canonical-uuid","max_bytes":67108864}
```

The hash and UUID strings above are descriptive placeholders. `max_bytes` must
be 1 through 67,108,864 inclusive. It counts snapshot bodies, metadata page
bodies and the final manifest, matching Bundle's admitted byte budget. Framing
has a separate bound: at most 2,049 item headers after the initial header.
There are at most 1,024 distinct snapshots and 1,024 distinct pages. No
per-session map or directory discovery is involved.

Each item header is followed by exactly the declared number of raw bytes and
one additional LF delimiter. Raw bytes may themselves contain LF. Never encode
the body as an escaped JSON string or base64.

| Bundle item | Header | Body bound |
| --- | --- | --- |
| Snapshot | `{"t":"snapshot","reference":{"sha256":"digest","bytes":N}}` | 1 MiB |
| Page | `{"t":"page","reference":{"sha256":"digest","bytes":N}}` | 1 MiB |
| Final record | `{"t":"record","bytes":N}` | 256 KiB |

Digests are lowercase SHA-256 hex over the exact raw bytes. Every body is
nonempty. Snapshots must be valid JSON. A snapshot must precede each page that
references it; deduplicated snapshots may serve multiple pages. Each page is
shared `SnapshotPage` v1, whose exact schema/source hash must match its manifest
source. Page indexes start at zero and increase by one within each family.
The final `ImportRecord` must match the starting configuration/import UUID,
contain precisely the received page references in order, have
`evidence:"staged"`, and retain `automatic_approvals:false`. All supplied
snapshots must be referenced. Exactly one final record ends the stream;
missing records, duplicate bodies/pages, extra bytes and missing EOF refuse.

The publisher buffers one bounded body, small bounded metadata/ref collections,
and at most three 64 KiB input chunks. It stages private files without holding
`handoff.lock`, so slow input cannot block revocation. Source readers/rechecks
must already enforce G3's exact original identity and unchanged-source rules.
Transport parsing cannot substitute for them.

## Commit and result

All output is under the selected private `desktop/imports` directory. Body
names are fixed `snapshots/<digest>.json`, page names are fixed
`pages/<digest>.json`, and the manifest is `record-v1.json`. No input contains
a destination path. Existing private-directory and no-follow single-link file
checks apply; unsafe leaves are refused without repair.

After complete input, validation and file sync, the publisher takes the short
OS-backed `handoff.lock`. A held active-writer/import lock refuses publication.
An exclusive UUID directory creation reserves that import ID permanently.
Private synced snapshot/page directories move into it, and the final synced
manifest is published last; parent directories are synced before success.
An existing UUID is never overwritten, including a partial directory left by
failure. A partial directory without a manifest remains ineligible. Source
stores and any existing handoff marker are untouched. No `owner.lock` or
desktop SQLite database is created by the publisher.

Exit zero prints one bounded JSON result: `abi`, `published:true`,
`evidence:"staged"`, exact `configuration` and `import_id`,
`import_record:{version:1,sha256}`, admitted body `bytes`, and snapshot/page
counts. Compare all identities before accepting the result. Reports contain no
snapshot bodies or source paths. Errors are nonzero and redacted.

There is one closed negative response: after all input and the final manifest
have been validated, contention from the actual `try_lock_exclusive` call may
return exit zero with `published:false`, `code:"handoff_busy"`,
`this_attempt_not_committed:true`, and the exact ABI, configuration, import UUID
and `import_record:{version:1,sha256}`. This invocation has not attempted the
UUID directory reservation. Path, open, privacy, identity and permission errors
are not this response. Any failure after reservation remains unknown.

Accept that negative only after the original child exits successfully and its
input writer and both output pipes finish completely, and only against the
exact expected manifest receipt retained before sending the final record. It
does not establish global absence or settle an earlier unknown attempt. A new
**explicit** attempt may use the same retained UUID and record after fresh
same-source checks. Lost, partial, malformed or mismatched negatives grant no
retry; neither does an ordinary exit 1 or an absent status result.

After a lost response, inspect the same ID using the supported read-only command:

```sh
hgs desktop import-status --json --import-id UUID
```

This checks one fixed manifest, never scans pages or directories and never
creates state. `record_present:false` also covers an incomplete UUID directory.
A present valid manifest reports its exact reference/evidence and page count;
it does not prove drain, previous sync durability, current delegation or effect
authority. An absent or unknown outcome must not silently generate another ID
or overwrite the old one. Preserve original sources and surface the result for
review. Native Windows filesystem/runtime qualification remains separate from
cross-target type checking.

## Native Qt/Settings report caller

The live switch identity's service connection captures managed sources off the
GUI thread after paired-CLI selection and configuration preflight, before the
startup Grant. Explicit Resume uses the same ordering. The report is best-effort:
busy, quarantined, unavailable and unconfirmed outcomes are recorded separately
and never prevent Grant, watch, catalog or normal preference edits. This first report slice
uses the existing Qt family decoders and the current local Settings bytes: 17
Qt families on Linux (16 on macOS), plus three paired Next policy/recovery
families, with at most 64 MiB, 1,024 snapshots and 1,024 pages. It is a private
Staged report, not materialization of these families into workers.

The Unix source adapter retains the original private, single-link Qt file and
Settings file identities and raw hashes. Mac defaults are exported read-only
through the fixed owned helper and re-exported before the final record; an
uncertain export never falls back to a plist. The local Settings writer pauses
new mutations during its original capture. The caller releases its capture after
the attempt; original child/pipe clones still retain it until actual completion.
This is source-specific exclusion,
not proof that Qt or the whole desktop drained. Every helper, writer and pipe
retains its original resources through actual completion. Shutdown reports
unconfirmed work rather than declaring an outstanding resource group drained.
The generic Windows Qt/Settings capture and private report-journal adapter
remain unavailable in this slice; the separate Windows pending-read checkpoint
is unchanged. Missing Qt is reported as not applicable without inventing an
empty source or a successful import.

The private managed `qt-retained-import-v1.json` retains the original UUID,
configuration, source hashes, attempt and expected manifest hash. Published
records are historical: later starts inspect the original ID instead of
re-seeding changed Qt state. Unknown attempts perform only original-ID lookup.
Only an exact known pre-reservation negative admits an explicit same-record
retry (at most 16 attempts). Invalid, changed or unsupported sources are kept
for review without a final manifest; no source is truncated, cleared or
rewritten. Service status carries body-free `retained_source_import` progress,
and the GUI shows deferred and review-needed reports independently of connection
readiness. Normal preference edits remain available after completed failures;
a later explicit known-negative retry must freshly capture the same source bytes.

This report does not replace the existing frozen pending-read capture or its
separate authenticated safe-passive page transfer. That transfer may wait for
the connected service and need not complete before Grant. If a prior runtime
holds the handoff lock (including a rollback hold), report publication is durably
deferred with `retained_import_handoff_busy` while the connection continues. It
does not post through the in-process lock owner or retry automatically. A later
explicit Retry may publish the same record only after an independently settled
handoff/revoke has freed that lock and same-source checks pass. The caller never
silently revokes a runtime or treats status as permission. Rearm makes at most
one report attempt; its following reconnect is not another report retry.
Remaining interim operation,
draft/cache source adapters and actual migration/upgrade acceptance stay open
under G3.
