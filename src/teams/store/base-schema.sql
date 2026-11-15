-- One immutable base per exact hand-out/reopen effect. No historical scan:
-- the first advance freezes the old default and an indexed effect high-water.
CREATE TABLE IF NOT EXISTS handout_bases (
 effect TEXT PRIMARY KEY NOT NULL, base_commit TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS base_origins (
 team TEXT PRIMARY KEY NOT NULL, base_commit TEXT NOT NULL,
 legacy_ceiling INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS base_advances (
 team TEXT NOT NULL, revision INTEGER NOT NULL, previous TEXT NOT NULL,
 commit_id TEXT NOT NULL, main_ref TEXT NOT NULL, PRIMARY KEY(team,revision)
);
