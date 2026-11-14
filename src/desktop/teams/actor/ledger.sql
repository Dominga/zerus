PRAGMA journal_mode=WAL;
PRAGMA synchronous=FULL;
PRAGMA max_page_count=16384;
CREATE TABLE IF NOT EXISTS actor_rows (
 team TEXT NOT NULL, key TEXT NOT NULL, version INTEGER NOT NULL,
 data TEXT NOT NULL CHECK(length(CAST(data AS BLOB))<=16384),
 PRIMARY KEY(team,key)
);
CREATE INDEX IF NOT EXISTS actor_delivery_node ON actor_rows(team,json_extract(data,'$.node')) WHERE key>='delivery:' AND key<'delivery;';
CREATE INDEX IF NOT EXISTS actor_delivery_member ON actor_rows(team,json_extract(data,'$.member')) WHERE key>='delivery:' AND key<'delivery;';
CREATE TABLE IF NOT EXISTS actor_budget (
 id INTEGER PRIMARY KEY CHECK(id=1), rows INTEGER NOT NULL, bytes INTEGER NOT NULL
);
INSERT OR IGNORE INTO actor_budget VALUES(1,0,0);
CREATE TRIGGER IF NOT EXISTS actor_insert BEFORE INSERT ON actor_rows BEGIN
 SELECT CASE WHEN (SELECT rows>=8192 OR bytes+length(CAST(new.data AS BLOB))>33554432 FROM actor_budget WHERE id=1)
 THEN RAISE(ABORT,'actor budget full') END;
END;
CREATE TRIGGER IF NOT EXISTS actor_added AFTER INSERT ON actor_rows BEGIN
 UPDATE actor_budget SET rows=rows+1,bytes=bytes+length(CAST(new.data AS BLOB)) WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS actor_update BEFORE UPDATE OF data ON actor_rows BEGIN
 SELECT CASE WHEN (SELECT bytes-length(CAST(old.data AS BLOB))+length(CAST(new.data AS BLOB))>33554432 FROM actor_budget WHERE id=1)
 THEN RAISE(ABORT,'actor budget full') END;
END;
CREATE TRIGGER IF NOT EXISTS actor_updated AFTER UPDATE OF data ON actor_rows BEGIN
 UPDATE actor_budget SET bytes=bytes-length(CAST(old.data AS BLOB))+length(CAST(new.data AS BLOB)) WHERE id=1;
END;
CREATE TRIGGER IF NOT EXISTS actor_deleted AFTER DELETE ON actor_rows BEGIN
 UPDATE actor_budget SET rows=rows-1,bytes=bytes-length(CAST(old.data AS BLOB)) WHERE id=1;
END;
PRAGMA user_version=1;
-- Frozen hosted inputs have the CLI's 30 MiB input bound, separate from
-- compact actor state rows. The database's page bound still caps the store.
CREATE TABLE IF NOT EXISTS actor_inputs (
 team TEXT NOT NULL, key TEXT NOT NULL, version INTEGER NOT NULL,
 data TEXT NOT NULL CHECK(length(CAST(data AS BLOB))<=31457280),
 PRIMARY KEY(team,key)
);
-- A fixed retry record for the terminal entry, independent of row budgets.
CREATE TABLE IF NOT EXISTS actor_terminal_retry (
 id INTEGER PRIMARY KEY CHECK(id=1),
 failures INTEGER NOT NULL CHECK(failures BETWEEN 0 AND 4294967295),
 next_ms INTEGER NOT NULL,
 reason TEXT NOT NULL CHECK(length(CAST(reason AS BLOB))<=2048)
);
