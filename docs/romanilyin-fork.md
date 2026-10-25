# Roman Ilyin fork

This repository continues [WhiteWh/zerus](https://github.com/WhiteWh/zerus),
which builds on [ufna/zerus](https://github.com/ufna/zerus).
The initial source is WhiteWh's `whitewh/main` commit
`f31086364f0964c4910a861c385baec904c39b0f`, not its older default `main` branch.
Existing copyright notices, application identifiers, state directories and
conversation identities are preserved.

## Scope

- Require PowerShell 7 Core for native Windows shell operations.
- Represent lexical path identity separately from physical filesystem access.
- Pin CanonicalPath release `2026.10.24-1` to source commit
  `50b390675a146874b1555f361e491d1efeb393a7` and the Cargo lockfile.
- Connect a Windows client to Linux `hgs` using an explicit WSL distribution and
  user, with the existing native OpenCode 2 adapter on the Linux side.
- Keep OpenCode service lifecycle owner-managed in this connection mode.

The operating-system namespace is part of a path's identity. A Linux PID and
loopback address are checked inside Linux. Windows and WSL paths become aliases
only through a configured mapping. Lexical normalization does not authorize I/O
or replace symlink-aware filesystem checks.

## Distribution

Initial development builds have no fork update feed. Keep `ZERUS_FORK_FEED_URL`
unset when building this fork; the inherited WhiteWh publication workflow is
repository-gated and does not publish here. Upstream download and package links
elsewhere in the inherited documentation describe upstream releases, not this
fork. A source checkpoint is not a signed release or a full platform qualification.

## Source and task history

`origin` points to this repository, `whitewh` to WhiteWh/zerus and `upstream` to
ufna/zerus. Changes use ordinary commits and fast-forward pushes. Task history
uses the existing Beads workflow, with synchronization directed at this fork.
Build logs and local deployment settings stay in ignored directories.

## Compatibility boundaries

SSH peers and existing session identifiers retain their current contracts.
Path identity is introduced at explicit boundaries; existing saved conversations
are not silently re-keyed. A WSL connection uses Linux native paths when invoking
Linux tools. Its configuration must name the intended distribution and user;
there is no fallback to whichever distribution happens to be the default.

## Development interfaces

See [Windows and local WSL](windows-wsl.md) for connection setup, shell policy
and isolated checks, and [explicit path identity](path-identity.md) for the
versioned PathRef CLI. These are development interfaces; no automatic catalog
migration or stable release is implied.

## Fork CI coverage

Every push to this fork selects Windows qualification in addition to the
inherited Linux, Arch, macOS and shared checks. Manual platform selection and
PR path selection remain available. The extra coverage uses the existing
credential-free Windows jobs, read-only caches and pinned dependencies; it does
not enable publishing, signing or an automatic update feed. Local Windows-to-WSL
acceptance stays a separate explicit opt-in because hosted runners are not the
operator-selected distribution and user.

Integration tests preserve the deliberate WhiteWh differences: drive folder
descriptions can synchronize with exact machine identity while local UNC shares
stay private; an expired quota token is distinct from native sign-in; team
worker status is additive; DeepSeek reads bounded session rows, with a read-only
fallback for older adapters. These checks do not authorize native writes or
service management during inspection.

Build identity, update isolation and matching Windows/WSL delivery are described
in [fork candidates](fork-candidates.md). Candidate labels do not rename saved
applications or sessions and do not authorize public releases.

Explicit WSL file references can now open their mapped Windows original after
two-phase session-scoped native verification and bounded local handle/digest
checks. This is separate from downloading a copy and never marks the original
as downloaded. See [the reverse-opening contract and limits](path-identity.md#explicit-wsl-file-references-opened-as-windows-originals).
