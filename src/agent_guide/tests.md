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
  hgs test map --check              fails when a test file or area is unmapped
  hgs test map --write              rewrite .zerus/test-map.json from the build
  hgs test durations --record LOG --provider NAME   keep a run's times

Each run line is a command for its folder: run it through hgs team run or
hgs run so it shows as your work. A QtTest change inside test functions also
names the binary with only those functions. A file the map does not know runs
its whole area; a file in no area is named, never skipped. Tests marked manual
(real agents, hosts) run only by hand; tests built elsewhere run in the CI.
Every action takes --dir DIR, --team TEAM (the owner's override) and --json.

.zerus/tests.toml: version = 1; [map] file, durations, unmapped (report|all),
ignore; [full] where (here|ci|runner), policy (owner|standing|anyone), target;
[retries] flaky, quarantine_days; [budgets] grow_percent, suite_minutes;
[isolation] enabled, variables; [build_cache] enabled, tool; [merge_queue]
enabled; [resources.NAME] cpu, ram_mb, gpu, exclusive; and [[provider]] with
name, kind (cargo|ctest|pytest|unittest|gradle|npm|command), root, sources,
tests, manual, elsewhere, affected (build|paths|all), [[provider.paths]]
(files with targets or mentions), command ({targets}, {manifest},
{build_dir}), manifest, widen (none|dependants|transitive), build_dir,
resources, platforms, budget_minutes, disabled.
