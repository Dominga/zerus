Test policy for a team (U62)

A task runs only the tests its change touches: hgs test affected names them
with their time; run them with hgs team run (the command hgs guide tests
gives), never a full suite. Ask the lead before a run over 10 minutes.

A hand-out names this rule. A phase integration runs the affected tests of
the phase's range. The full run happens only before a release: ask the owner
first with the estimate; the owner sets the full_tests policy in the team
settings (never, ask, allowed) or grants one run for a release.

No test is dropped: a split keeps every test, counted before and after. A new
test that repeats an existing one becomes a case of a table test.
