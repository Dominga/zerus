# Desktop schema 35: preview and offline rollback

Schema 35 separates an immutable creation identity from its worktree scheduling
slot. The column migration, immutable triggers and `user_version=35` commit in
one SQLite transaction. An error or process exit before commit leaves the old
schema and rows intact. Startup reports the migration error and does not publish
a ready endpoint. It never continues with a partial migration.

A schema-34 `hgs` refuses schema 35 with `desktop state requires a newer hgs
version`, before a writable connection or migration. It leaves the database,
new columns and retained rows untouched, but its service cannot start. Installing
an older preview is therefore **not** an application-only rollback. After a
passing preview, merge this change immediately and build later previews on top
of it. Keep the previous CLI/GUI installation through the maintained updater.

## The one-time backup

Before the first migration of an existing store, the service holds its original
service-owner and handoff locks, closes its read-only version probe, and copies
the original private database plus its WAL (or records positive WAL absence).
The probe connection closes before copying. On Unix, existing raw inputs enter
the process-retained registry described below; none closes while another local
SQLite connection could own locks. Windows backup-only input handles close
before the writable SQLite connection opens; the original database pin follows
the Store connection lifetime. Native objects, versions and streaming digests
are checked before and after copying. Reads use 64 KiB chunks, a 30-second
aggregate deadline, 1 GiB per file and 4 GiB of aggregate read work including verification. No contents enter
the diagnostic log.

Let `D` be **this configuration's actual desktop state directory** (the
`desktop` child of its configured state root). The fixed backup is:

```text
D/schema-34-rollback/manifest.json
D/schema-34-rollback/state.sqlite3
D/schema-34-rollback/state.sqlite3-wal   # only when manifest.wal is non-null
```

The manifest records the source schema, time, configuration and original native
root/database identities, exact file sizes and SHA-256 values. The directory
and each file are private and published without replacement. Files are flushed;
Unix also syncs the directory and its parent. Windows uses the native publication
contract and limits below. The manifest is published last. An incomplete
directory cannot qualify: startup
refuses before migration and retains it for inspection. An operator may preserve
such an incomplete directory under another name **only after verifying the
original store is still schema 34**; the service never repairs or overwrites it.
An existing complete backup must validate its whole fixed roster, provenance,
sizes and hashes before it can support migration. Later starts never overwrite
the backup. Absence of a backup is not proof of no previous effects.

### Windows publication and durability limits

Windows publishes each file through the **same synchronous CREATE_NEW handle**
that wrote it. A native same-parent rename uses NOREPLACE for backup files; the
directory and ancestor handles continue denying write/delete sharing. No pathname
rename reopens that parent, no sharing permission is relaxed, and no retry is
introduced. The handle uses `FILE_FLAG_WRITE_THROUGH` and is flushed before and
after publication. Database/WAL content copied through the retained handle is
flushed again before validation; the manifest is still published last.

This relies on the native handle flush/publication contract, **not** the Windows
`platform::sync_dir` compatibility no-op. Microsoft documents [NTFS metadata
flushing for write-through requests, including renames](https://learn.microsoft.com/windows/win32/api/fileapi/nf-fileapi-createfilea)
and [file cache flushing](https://learn.microsoft.com/windows/win32/fileio/file-caching).
It is not a Unix directory-fsync guarantee on arbitrary filesystems, network
redirectors or hardware that does not honor flush/write-through. Native CI tests
process interruption and validates the retained bytes/manifest; it does not
simulate sudden power loss or certify every storage stack. Retain this residual
when planning rollback, and validate the complete matched backup offline.

An older failed Windows upgrade may already have left an incomplete
`schema-34-rollback` directory. A later start keeps refusing that directory,
including after the publication fix. **Do not delete or repair it automatically.**
Follow the offline checks below, verify the original database remains schema 34,
and retain the incomplete directory for review before an operator preserves it
under another name. A fresh backup requires the same verified original store and
exclusions; its absence never supplies historical effect evidence.

WhiteWh fork (task #421): an EMPTY leftover directory, with no manifest and no
file at all, holds nothing to review. The start makes the complete backup in
it, with the same NOREPLACE publication and the manifest last; any other
content keeps the refusal above.

## Coordinator-operated rollback

This is an offline recovery procedure, not an automatic action or a request to
perform it on an installed machine.

1. Close the GUI through the maintained updater, retaining drafts. Stop new
   desktop admission through its verified revocation/drain protocol. Confirm
   every original desktop child, pipe, loan and custody charge has physically
   settled. Stop only the desktop service through its owning service manager.
   Do not stop native agents, tmux servers or a DeepSeek host. If an original
   effect or its retirement remains unknown, do not roll the ledger back.
2. Verify the desktop service and every other SQLite user of `D/state.sqlite3`
   is closed. Keep the exact original configuration and state path. Preserve
   the current `state.sqlite3`, `state.sqlite3-wal` (when present) and
   `state.sqlite3-shm` separately for diagnosis; do not merge their bytes with
   the backup. Do not remove owner or handoff exclusion files.
3. Validate the manifest and both retained files' sizes and hashes, without
   opening the retained backup itself in SQLite. Opening it can create a new
   SHM file. For inspection, first copy the pair to another private directory.
   Require the recorded configuration and original store/root provenance.
4. Restore `schema-34-rollback/state.sqlite3` to `D/state.sqlite3` as a private,
   single-link regular file. If `manifest.wal` is non-null, restore its matched
   `state.sqlite3-wal` beside it. If and only if the manifest positively records
   `wal:null`, remove the current WAL instead. Discard only the stale
   `D/state.sqlite3-shm`; SQLite rebuilds it. Flush the restored files and, on
   Unix, sync `D`. Keep the Windows durability limits above when restoring there.
   Keep the backup untouched.
5. Restore the previous `hgs` and GUI through the maintained installer/updater;
   never stage an application copy by hand. Start the desktop service with the
   same configuration, verify its source/version and IPC readiness, and verify
   the restored schema and original rows before reopening the GUI. Reconcile
   any outstanding external receipts and custody before enabling writes; a
   rollback never authorizes replay of work absent from the restored ledger.

**Loss window:** all desktop-ledger state written after the manifest's backup
time is lost on rollback. CLI worktree receipts, the creation journal and other
external state are not rolled back with this database. If their newer effects
cannot be reconciled conservatively, retain the newer installation and inspect
instead of restoring the older ledger.

## The previously stuck creation card

The preview preserves the card and every original request ID. An agent does not
press Stop or Recreate on the owner's machine. After installation, the owner:

1. Presses **Stop creating**. If the original missing request is successfully
   fenced, the card shows **Creation stopped**, says that further starts are
   fenced and the earlier outcome is unknown, and keeps its original UUID.
   A failed transport or unverifiable store remains Unknown and displays its
   reason; it never claims no past effects.
2. Presses **Recreate…**. This opens the normal reviewed creation choices for a
   **new creation identity**. The owner reviews the destination/name and presses
   **Start session** to admit its fresh worktree UUID. The old request is never
   resumed. Hide status can dismiss its presentation without erasing receipts.

The service's status and diagnostic reads remain read-only throughout this
check. No manual deletion of the owner's folder, branch or journal is needed.

## Passive startup while an original handoff is held

A current-schema store can start passively under its original service-owner
lock while an import or installer retains `handoff.lock`. A missing database,
or a positively empty version-zero database without ambiguous sidecars, can
perform initial passive preparation under that same owner. The exact database,
schema, configuration and empty shape are checked again before writable work.
Existing-data migrations still require the original handoff exclusion, a fresh
observation under it, and the verified rollback backup described above.

On Unix, each opened original database or existing backup-input descriptor stays
in the same fixed process-wide registry of at most **64** pins until process
teardown. Even a failed opener retains its descriptor: closing it could release
another local SQLite reader's record locks. Reuse checks the held original and native name; schema results are
never cached. The cap refuses before opening another descriptor. A normal
service uses one selected database and its bounded rollback-input roster.
Windows retains its original handle through the Store connection instead, using
its handle-scoped lock lifetime. Backup copying, hashing and manifest parsing use positional reads on the retained
originals. Name and sidecar-absence checks use metadata only, so a rejected
alias cannot close a raw descriptor of another live SQLite database.
