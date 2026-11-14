These are synthetic backups produced at base
`69d317274e3315f5befaac8315e237b74e1607e1` for zerus-63rru.132.
They are generated data, not hand-authored team, launch or binding rows.

`Harness::new` calls the product Create operation. The launch runtime advances
its normal ledger and SetMemberSession writers, with the existing FakeEffects
port providing synthetic accounts, checkout observations and session identities.
Thus the saved branch uses that base fixture port's `team-<launch>` spelling;
the destination and session tag retain their pre-upgrade UUID spelling.
The supervisor's `listing_fixture::launched_record`, `confirmed`, `save` and
`rename` writers create the session records and ID indexes. The member is renamed
to `claude/example/person-renamed`, then another product-written session takes
its former label. No live agent or personal state is used.

SQLite VACUUM images are exported with Python sqlite3.iterdump, preserving the
schema version. JSON record files are copied byte for byte. The upgrade test
restores these backups and verifies the saved original, placement and binding;
its session reader must select the original ID despite the reused label.
The new-name worktree test separately exercises the real CLI dispatcher and
reads its product-written ownership receipt.

The temporary producer and generation log are retained for review under
`artifacts/test-results/wp132/` (`generate-base.py`, `retain-base.py`,
`base-writer.log`). The producer was run with only its exact test selected and
one test thread. The base source is otherwise unchanged.
