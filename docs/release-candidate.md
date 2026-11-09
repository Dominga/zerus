# The release candidate, and why whitewh/main only moves through it

Phase 3's release went around the build system: after local member test runs the
lead pushed `whitewh/main` by hand. Nothing in the build system required the
release pipeline to run before publishing, and there was no target for the
release steps or for the GitHub runner's environment (Visual Studio 2026 only,
its other tool versions). Local tests on a member folder cannot see that
environment.

Two legs now stand between a commit and `whitewh/main`:

1. **Local release candidate** — `hgs team run` template `release-candidate`
   runs `scripts/release-candidate.py` on the Windows worker. It runs the steps
   of `fork-windows.yml` after the Rust suite, without Qt (task #388):
   `fork_update`, `fork_handoff`, the Tauri build with the fork channel
   (`scripts/release-candidate-local.py`), `package-windows.py` and
   `check-windows-portable.py --run --selftest`. A green result is recorded in
   `.zerus-release/<commit>.json`.
2. **Runner leg** — the same script with `--exp-tag <name>` creates and pushes
   `whitewh-exp-<name>` for the exact commit, waits for the `Fork Windows build`
   workflow on that tag, and records its green conclusion. That tag publishes
   an experimental prerelease, never the latest release, no feed.

## Pre-flight by hand

```
python scripts/release-candidate.py --commit <sha> --exp-tag rc-<date>-<n>
```

The script refuses to continue a leg that fails and never records a failure as
green. Its local leg is also the `release-candidate` template, which a member
runs without an approval; the template never takes `exp_tag`, because the
runner leg pushes a tag and stays the owner's step by hand:

```
hgs team run --template release-candidate --args '{"commit":"<sha>"}'
```

The local leg takes the fork identity too (task 213). The runner gets its fork
build number (the CI run number) and feed URL from the tag that starts
`fork-windows.yml`; a local leg has no tag, so pass both explicitly:

```
hgs team run --template release-candidate --args '{"commit":"<sha>","fork_build":"<next CI sequence>","fork_feed":"https://github.com/WhiteWh/zerus/releases/latest/download/zerus-fork-feed.json"}'
```

`fork_build` is the number the build publishes (the next CI sequence, the number
the same experiment's workflow would carry); `fork_feed` is the fork's update
feed. Both are embedded by the Tauri build (`ZERUS_FORK_BUILD` and
`ZERUS_FORK_FEED_URL`), named by the packaged `build-info.json`, compared by
`check-windows-portable.py --run --selftest`, and recorded under `fork` in
`.zerus-release/<commit>.json`. Give both or neither: an experimental leg with
neither builds without a fork identity, and the self-test then compares "no
build, no feed". The scripts refuse one without the other, a build that is not a
positive number and a feed outside `https://github.com/WhiteWh/zerus/releases/`.

Task #388 (owner, 2026-11-08): the local leg runs no Qt step. It records each
step's seconds (`timings_s`), the whole leg (`total_s`) and the zip's size
(`zip_bytes`) with its result in `.zerus-release/<commit>.json`, and prints
them at the end.

## Publishing

`scripts/publish_whitewh_main.py --commit <sha>` refuses unless
`.zerus-release/<sha>.json` names the same commit and both `local` and
`experimental` are `green`; only then does it push `<sha>:refs/heads/whitewh/main`.
Do not push the release branch any other way. The gate's unit tests are in
`scripts/test_publish_gate.py`.

## Machine-dependent tests

`scripts/check-machine-dependent-tests.py` scans Rust test functions that run
Visual Studio, an Unreal engine, the registry, a GPU tool or the network. A
test must probe (`available`, `probe`) and skip, or be `#[ignore]`d with the
reason; assuming the tool is installed is a review rejection. The release
candidate runs the scan before it builds.
