Run only the tests your change touches (hgs guide tests)

.zerus/tests.toml sets rules; the owner may override them in team settings.
Without it Zerus only proposes detected tests and enforces nothing.

  hgs test affected                 uncommitted changes, with time
  hgs test affected BASE..HEAD      a range; --staged for the index
  hgs test affected --files A B     named files
  hgs test affected --full          the whole set and time; only the owner
                                    authorizes a full run
  hgs test config                   providers, areas and full-run policy
  hgs test map --check              fails on unmapped files/tests
  hgs test map --write              refresh the map snapshot
  hgs test durations --record LOG --provider NAME   record run times
  hgs test tools                    tools needed here

Sources refresh the map; its snapshot covers unavailable builds. Only tracked
files count: git add new tests. Each run line names its command and folder.
Send printed hgs team run requests, build first: cargo-test, qt-test,
cmake-build/ctest, cython-build/py-test, msbuild-build/vstest, ue-build/ue-test.
hgs run takes other kinds. QtTest function changes name binary:function.
Unknown files run their whole area; files in no area are named, never skipped.
Manual tests (real agents, hosts) run by hand; elsewhere tests run in CI.
Every action accepts --dir DIR, --team TEAM (owner override) and --json.
Never install a tool: propose it on the question card (Waiting for you),
then work without it.

Windows private-store fixtures use owner_only_dir on a missing tempdir child.
Keep existing ACLs unchanged; never repair Temp or weaken privacy checks.

Local merge-queue CTest probes, configures missing builds, always builds, then
tests under one deadline. Build failures are not retried as flaky tests.
Machine vars/env stay in untracked .zerus/tests.local.toml; failed probes are
not run here, never green. Owner overrides apply to every stage; remote
runners are unchanged.

Config: [map], [full], [retries], [budgets], [isolation], [build_cache],
[merge_queue], resources and providers (cargo, ctest, pytest, cython, unittest,
gradle, npm, msbuild, unreal, command); paths, manifest, build_dir and all
fields: docs/reference.md, Affected tests.
