Run only the tests your change touches (hgs guide tests)

A project says how in .zerus/tests.toml; the owner may override it in the team
settings. Without the file Zerus only proposes what it detects, nothing is
enforced, and the team keeps today's flow.

  hgs test affected                 tests for your uncommitted changes, with time
  hgs test affected BASE..HEAD      the same for a range; --staged for the index
  hgs test affected --files A B     the same for named files
  hgs test affected --full          the whole set and its time (never run it:
                                    the full run is the owner's call)
  hgs test config                   providers, areas and the full-run policy
  hgs test map --check              fails when a tracked file is in no area or a
                                    test file in no provider's map
  hgs test map --write              refresh the committed snapshot of the map
  hgs test durations --record LOG --provider NAME   keep a run's times
  hgs test tools                    the tools the tests need here

The map is read from the sources when you ask, so a merge never leaves it
behind; the committed file stands in only for a build this checkout cannot
read (the Qt build in the CI). Only tracked files count: git add a new test.
Each run line is a command for its folder. A team member sends the printed
"hgs team run" request (cargo-test with filters and skips, qt-test with
suites and suite:function); hgs run takes the other kinds. A QtTest change
inside test functions also names the binary with only those functions.
A file the map does not know runs its whole area; a file in no area is named,
never skipped. Tests marked manual (real agents, hosts) run only by hand;
tests built elsewhere run in the CI.
Every action takes --dir DIR, --team TEAM (the owner's override) and --json.
Never install a tool: Zerus offers a missing one to the owner (Waiting for
you). Propose it on the question card and work without it.

.zerus/tests.toml: [map], [full] (where, policy), [retries], [budgets],
[isolation], [build_cache], [merge_queue], [resources.NAME] and [[provider]]
(name, kind cargo|ctest|pytest|unittest|gradle|npm|command, root, sources,
tests, manual, elsewhere, affected build|paths|all, [[provider.paths]],
command, widen, build_dir); every field: docs/reference.md, Affected tests.
