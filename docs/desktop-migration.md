# Desktop import evidence and writer handoff

G1 (`hgs desktop`) and G3 (read-only migration) agreed this v1 contract on
2026-10-30. The shared Rust crate is `crates/desktop-migration`, package
`zerus-desktop-migration`. It contains structural types and pure validation,
not an importer, effect worker or ownership proof. Existing Qt/interim readers
remain the import stack. See the [migration inventory](superpowers/specs/2026-10-29-qt-retirement-inventory.md#migration-contract)
and [service design](superpowers/specs/2026-10-29-hgs-desktop-service-design.md#handoff-coexistence-and-upgrade).

## Marker

`desktop/handoff-v1.json` below the existing `HGS_STATE_DIR` is at most 64 KiB:

```json
{
  "version": 1,
  "configuration": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "source": "com.hgdev.hgs-tray",
  "import_id": "11111111-1111-4111-8111-111111111111",
  "writer_epoch": "22222222-2222-4222-8222-222222222222",
  "phase": "imported",
  "import_record": {"version": 1, "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
  "automatic_approvals": false
}
```

`Handoff::parse` rejects missing, unknown and malformed fields, unsupported
versions, wrong configuration/source, noncanonical UUIDs, non-lowercase hashes,
and migrated approval consent. Development/local identities and replay never
participate. `writer_epoch` is the durable cutover generation, distinct from a
service process/watch epoch, and authorizes no delivery replay.

The closed v1 phase enum is `imported`, `quiesced`, `complete`, `revoked`.
`revoked` durably withdraws the service's migrated writer authority before a
rollback or Qt start. G1 alone commits it. Reason/history stay in G1's private
journal, without extending the marker payload. Older consumers fail closed on
the unknown phase; none may interpret it as a new import or reseed.

The import record always resolves to
`desktop/imports/<import_id>/record-v1.json`; JSON cannot supply another path.
`Handoff::verify_import_record` checks the exact byte digest, version and matching
configuration/import identity. Missing or mismatched evidence leaves G1 passive
and is never repaired by rewriting or replacing it with defaults.

## Immutable import evidence

`ImportRecord` is a metadata-only manifest, at most 256 KiB. Its exact fields are
`version`, `configuration`, `source`, `import_id`, `evidence` (`staged` or
`drained`), `created_at` (Unix seconds), `automatic_approvals` (false), `sources`
and `pages`. Each source records a family, reader schema version, format, exact
source-byte SHA-256 and optional verified build identity. There are at most 256
source records with distinct family names and 1024 distinct page references.

Each page reference has only `sha256` and `bytes` (1 to 1 MiB) and resolves to
`desktop/imports/<import_id>/pages/<sha256>.json`. `verify_page` checks exact
length/digest. Private pages carry bounded family metadata, source paths, schema
and snapshot/quarantine references. The manifest and handoff marker contain no
message, question, token or session arrays. No credentials are copied from their
privileged stores. Snapshots and quarantine details are never repository data.

G3 stages read-only snapshots through the existing importers. Before old writers
drain, evidence is explicitly `staged` and ineligible for takeover. G3 publishes
immutable `drained` evidence only after the drain; it never changes a committed
import. These fields describe a past import, not current process ownership.
Automatic-approval reset evidence must inspect both Qt's former choice and
Next's interim policy. Neither carries consent to the service.

Previously unhandled families may import once under the versioned first-G3 rule
in the inventory. Next's existing values win. A family record includes schema,
source path, time, key names and the exact source-byte hash; a changed Qt source
since the original switch is recorded and remains eligible for a new family.
Failed/partial families remain unhandled. Already handled settings/layout/drafts
never re-seed after rollback.

## Locks and live proof

G1 retains `desktop/owner.lock` for its process lifetime. G3 never takes it.
`desktop/handoff.lock` is a separate OS-backed exclusive lock: G3 holds it only
to commit immutable evidence and refuses/retries while G1 holds it. G1 rereads
the exact import UUID and digest under that lock, owns all marker transitions
(`imported`, `quiesced`, `complete`, `revoked`), and retains the lock while any migrated
writer runs. Commits use atomic replacement with durable file/directory flushes.

Readers/committers require private directories and private regular single-link
files, no-follow opens and byte limits. Pure validation does not perform these
filesystem checks. A complete marker never replaces fresh runtime proof:
versioned build/configuration/writer-epoch hello, stopped old admissions, durable
drain, exact shared effect identities and both service locks. The revised
coordinator ruling of 2026-10-30 uses explicit durable revocation before rollback
or Qt start on the normal path, with live legacy-GUI detection revoking the
abnormal path. It does not require the service to own Qt's Unix socket; the
temporary instance start lock alone cannot prove long-lived exclusion. Takeover
remains fenced until revocation and exact effect guards exist. Windows permanent
lock exclusion is separately unqualified. Arch remains passive until the explicit
Linux handoff even if a marker is copied there.

`configuration(state, config)` extracts the service's existing algorithm:
canonicalize existing path ancestors, then SHA-256 the state and config paths in
that order, each native encoded byte string prefixed by its little-endian u64
length. Missing descendants are resolved without creating directories. G1 and
G3 must call this same helper; lexical paths or independently approximated hashes
cannot establish configuration identity.

## Delivery boundary

The shared crate validates no running process and offers no `ReadyForWrites`
boolean. It writes no marker, lock, source store or registration. G1 composes
structural results with fresh runtime evidence before taking writer authority;
the G3 importer and G1 integration are separate implementation slices. Client
settings, local layout and drafts retain their existing owners. G1 takes no
connector, draft or run-layout writer and never restarts agents or other services.
