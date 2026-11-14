# Native macOS leftover-removal qualification

macOS leftover deletion remains disabled. No replacement-preserving removal
guard was established by the authorized native qualification of 2026-11-14
for zerus-ty94.6 and zerus-ty94.6.2. Keep the existing pre-effect refusal,
`worktree-leftover-delete-v1` kept outcome, review and Keep folder behavior.

The qualification used macOS 27.2 / Darwin 27.2.0 on arm64, with source
`cabae1c85`, a private clone, synthetic directories, the shared heavy lock and
immediate removal of private targets. The raw controls checked the named inode
against a held directory fd and enumerated that held directory as empty. The
swap or insertion occurred after those final checks, immediately before the
syscall. No real worktree or existing user folder was a removal target.

| Candidate | Valid empty control | Late root and nested replacement |
| --- | --- | --- |
| `unlinkat(parent, name, AT_REMOVEDIR)` after identity and empty census | Removed | Removed arbitrary `._payload` bytes and the replacement |
| The same call with `AT_NODELETEBUSY` | Removed | Also removed the unreviewed bytes and replacement |
| `unlinkat(held, ".", AT_REMOVEDIR)` | `EINVAL` | Refused; cannot perform the required empty removal |
| `unlinkat(held, "", AT_REMOVEDIR)` | `ENOENT` | Refused; cannot perform the required empty removal |
| The empty-path call with `AT_FDONLY` | `EINVAL` | Refused; cannot perform the required empty removal |

An ordinary nonempty file was preserved with `ENOTEMPTY`, establishing the
contrast with the arbitrary AppleDouble-prefixed name. Adding `._payload` to
the same reviewed inode after an empty census also lost its bytes at both
root and nested names. A final census therefore does not close the gap, and
an fd-only operation that cannot remove a valid empty directory is not a
usable deletion guard. These observations do not claim that every possible
native strategy has been ruled out; they do not justify re-enablement.

## Repeating the native checks

Use an explicitly authorized native window, a private clone below
`~/zerus-dev`, synthetic temporary directories and the shared
`~/zerus-dev/.heavy.lock`. Check free space first. Clear inherited `HGS_*`, use
four Cargo jobs and a private target and temporary directory, and remove the
target immediately after the native run. Never displace another lock holder
or use an existing worktree or folder as the canary.

The existing ignored test
`state::worktree_cleanup::tests::macos_late_swap_preserves_appledouble_payload_at_root_and_nested_names`
asserts the containment refusal and byte preservation. Run it with
`--exact --ignored`. Its passing result proves that the wrapper refused before
the destructive syscall; it does not prove a raw primitive is safe.

The separate ignored test
`state::worktree_cleanup::tests::macos_raw_appledouble_observations_do_not_qualify_deletion`
must also be selected explicitly with `--exact --ignored --nocapture`. It
intentionally calls raw `unlinkat` only on its freshly created synthetic
directories. Its empty and ordinary-file controls must pass, and it prints
`APPLEDOUBLE_KERNEL_OBSERVATIONS` for late root and nested swaps and insertions.
Read the observed `payload_intact` independently of the return code. A passing
observation test is not evidence for a safe production guard.

The native syscall evidence is retained in the ignored artifact directory
`artifacts/test-results/macos-leftover-guard/`. The production identity, mount,
owner, bounded-walk and kept-v1 contracts remain unchanged. Re-enablement needs
a separately verified guard that preserves unreviewed replacement bytes at
both removal sites while still removing a valid reviewed empty directory.

The native kept-v1 integration control seeds one private legacy partial-removal
record directly. Its precondition is a reviewable leftover, independent of
whether a different Git-removal request passes native review. It still checks
bytes, permissions, the unchanged removal record, no consumed attempt, the kept
branch and the explicit Keep folder action.
