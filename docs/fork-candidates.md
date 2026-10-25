# Fork candidates and matching Windows/WSL delivery

`FORK_VERSION` is the independent `YYYY.M.D-N` identity of this fork. The desktop
shows **Fork ... candidate**; upstream desktop/CLI semantic versions remain
unchanged for compatibility. Application names, settings paths, native IDs and
session identity formats do not change. A candidate is not a signed or published
stable release.

## Embedded provenance

`hgs version --json` reports the fork distribution, candidate version/channel,
exact source commit, dirty flag, OS/architecture and supported WSL, PathRef and
existing-conversation contracts. Plain `hgs --version` keeps its legacy output.
Git-less source builds report unknown/dirty provenance and cannot pass the paired
candidate packaging gate. Build input and Git-ref watches keep cached builds from
claiming a different committed source.

`hgs-tray --build-info` includes the corresponding distribution, fork version,
channel and update-channel state. Windows packaging executes the exact staged
CLI and desktop with a disposable profile, checks their source/version against
the clean checkout, and records both identities in `build-info.json`. The portable
runtime checker verifies them again in its minimal environment.

Production fork candidates never read the upstream update catalog or reuse a
cached upstream catalog as a fork release. `ZERUS_FORK_FEED_URL` must be unset;
a nonempty value fails configuration. No publisher, signing key, repository
secret or external package channel is enabled by these changes. The inherited
upstream comparison tests use explicit local fixture transports.

## Assemble a matching candidate

Build both executables from the same clean source commit with the pinned Rust
and Qt dependencies. Produce and validate the Windows portable zip first. On
Linux, with the matching release receiver and cached locked Cargo dependencies:

```sh
RUSTUP_TOOLCHAIN=1.99.1 python3 scripts/package_fork_candidate.py \
  --windows-zip /path/to/verified-windows.zip \
  --linux-receiver /path/to/matching/hgs \
  --output /path/to/new-candidate-directory
```

Assembly refuses an existing output folder. It checks the receiver ELF target and
executes only that explicitly selected receiver to obtain its build identity,
using a disposable HGS profile. It collects the exact Linux dependency notices,
then atomically publishes the local directory only after verification. It does
not install anything, create sessions or start/stop an agent service.

The result contains `windows/zerus-windows-x86_64.zip`, `linux/bin/hgs`,
the legacy-compatible `linux/bin/hgs_state.py`, `linux/receiver.json`, Linux notices, a readme and `fork-candidate.json`. The latter
binds all component files by size and SHA-256 to one source/version and records
the Linux libc used by the builder. It is not a guarantee of compatibility with
older Linux distributions; qualify the target system separately.

Verify without executing a binary on any platform:

```sh
python3 scripts/package_fork_candidate.py --verify /path/to/candidate
```

Verification rejects unsafe or duplicate paths, symlink traversal, unlisted files,
size-limit violations, modified artifacts, mixed/dirty/foreign build identities
and claims of signing/publication. Integrity is not publisher authenticity:
record the printed manifest SHA-256 separately and trust it only through an
approved delivery channel.

## Installation and rollback boundary

Stage the Windows archive in a new versioned folder and the Linux receiver in
its own matching versioned location. Keep both previous components and the prior
explicit connection configuration together. Update the connection/launcher only
after comparing the complete source identities and running the isolated/runtime
gates. A rollback selects the previous pair; it must not restart OpenCode, replace
native history or rewrite session/project IDs. The normal application installation
and shared state identifiers remain independent of the candidate label.

Interactive acceptance and an owner-authorized release/update channel are still
separate gates. The packaging tools deliberately perform no automatic deployment
or background update.
