-- Product writers at main 6ef5d093c (Dev 3578 nested-plan code).
BEGIN TRANSACTION;
CREATE TABLE budget_alerts (
    team TEXT NOT NULL,
    member TEXT NOT NULL CHECK(member='@budget'),
    version INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(team, member)
);
CREATE TABLE journal (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    team TEXT NOT NULL,
    member TEXT NOT NULL,
    version INTEGER NOT NULL,
    at_ms INTEGER NOT NULL,
    actor TEXT NOT NULL,
    state TEXT NOT NULL,
    UNIQUE(team, member, version)
);
INSERT INTO "journal" VALUES(1,'00000000-0000-4000-8000-00000000beef','@branch',0,1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','"retry"');
INSERT INTO "journal" VALUES(2,'00000000-0000-4000-8000-00000000beef','@branch',1,1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','"branch_done"');
INSERT INTO "journal" VALUES(3,'00000000-0000-4000-8000-00000000beef','lead',0,1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','"retry"');
INSERT INTO "journal" VALUES(4,'00000000-0000-4000-8000-00000000beef','lead',1,1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','"worktree_intent"');
INSERT INTO "journal" VALUES(5,'00000000-0000-4000-8000-00000000beef','lead',2,1234,'{"kind":"service","person":"local","team":"00000000-0000-4000-8000-00000000beef"}','"worktree_ready"');
CREATE TABLE launch_budget(id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 16384),bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 67108864));
INSERT INTO "launch_budget" VALUES(1,2,2092);
CREATE TABLE launch_checkout_objects (
            team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL,
            data TEXT NOT NULL CHECK(length(CAST(data AS BLOB))<=16384), PRIMARY KEY(team,member,launch));
CREATE TABLE launch_health (
    id INTEGER PRIMARY KEY CHECK(id=1),
    code INTEGER CHECK(code IS NULL OR (typeof(code)='integer' AND code IN (1,2))),
    at_ms INTEGER CHECK(at_ms IS NULL OR (typeof(at_ms)='integer' AND at_ms>=0)),
    CHECK((code IS NULL)=(at_ms IS NULL))
);
INSERT INTO "launch_health" VALUES(1,NULL,NULL);
CREATE TABLE launch_team_names (
    team TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE CHECK(length(slug) BETWEEN 1 AND 48 AND slug NOT GLOB '*[^a-z0-9-]*')
);
INSERT INTO "launch_team_names" VALUES('00000000-0000-4000-8000-00000000beef','fixes');
CREATE TABLE notice_budget(id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 16384),bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 67108864));
INSERT INTO "notice_budget" VALUES(1,0,0);
CREATE TABLE originals (
    team TEXT NOT NULL,
    member TEXT NOT NULL,
    version INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(team, member)
);
INSERT INTO "originals" VALUES('00000000-0000-4000-8000-00000000beef','@branch',1,'{"team":"00000000-0000-4000-8000-00000000beef","member":"@branch","epoch":"00000000-0000-4000-8000-000000000001","person":"local","version":1,"original":"87365246-18f0-4cda-b20c-2ef2d807369c","observed":null,"parent_result":null,"state":"branch_done","plan":null,"branch":{"team":"00000000-0000-4000-8000-00000000beef","project":"example-project","machine":"00000000-0000-4000-8000-0000000000aa","branch":"zerus/team/fixes","base":"0123456789abcdef0123456789abcdef01234567","placement":{"source":"/workspace/example","git_dir":"/workspace/example/.git","branch":"team-87365246-18f0-4cda-b20c-2ef2d807369c","destination":"/workspace/zerus-team-87365246-18f0-4cda-b20c-2ef2d807369c"}},"worktree":null,"problem":null,"card_request":null,"card_done":false,"card_id":null,"retry_at":0,"failures":0,"retry_seen":false,"parent_tries":0,"unknown_until":0,"late_after_stop":false,"placement":null}');
INSERT INTO "originals" VALUES('00000000-0000-4000-8000-00000000beef','lead',2,'{"team":"00000000-0000-4000-8000-00000000beef","member":"lead","epoch":"00000000-0000-4000-8000-000000000001","person":"local","version":2,"original":"1eb663d8-921b-4de6-a97c-d15a606c1890","observed":null,"parent_result":null,"state":"worktree_ready","plan":{"id":"66b2464c-d247-4b89-929b-1f904bc74c2e","team":"00000000-0000-4000-8000-00000000beef","person":"local","machine":"00000000-0000-4000-8000-0000000000aa","project":"example-project","member":"lead","role":"lead","agent":"claude","account":"a","model":"","effort":"","base_commit":"0123456789abcdef0123456789abcdef01234567","placement":{"source":"/workspace/example","git_dir":"/workspace/example/.git","branch":"zerus/team/fixes/lead","destination":"/workspace/zerus-team-fixes-lead"},"parent":null,"launch_id":"1eb663d8-921b-4de6-a97c-d15a606c1890","session_name":"fixes-lead"},"branch":null,"worktree":null,"problem":null,"card_request":null,"card_done":false,"card_id":null,"retry_at":0,"failures":0,"retry_seen":false,"parent_tries":0,"unknown_until":0,"late_after_stop":false,"placement":{"source":"/workspace/example","git_dir":"/workspace/example/.git","branch":"zerus/team/fixes/lead","destination":"/workspace/zerus-team-fixes-lead"}}');
CREATE INDEX journal_team_sequence ON journal(team, seq);
CREATE INDEX launch_settled ON originals(team,json_extract(data,'$.state'));
CREATE TRIGGER launch_insert AFTER INSERT ON originals BEGIN UPDATE launch_budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER launch_update AFTER UPDATE OF data ON originals BEGIN UPDATE launch_budget SET bytes=bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER launch_delete AFTER DELETE ON originals BEGIN UPDATE launch_budget SET rows=rows-1,bytes=bytes-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER launch_journal_bound AFTER INSERT ON journal BEGIN DELETE FROM journal WHERE seq <= NEW.seq-10000; END;
CREATE TRIGGER launch_insert_guard BEFORE INSERT ON originals WHEN NOT EXISTS(SELECT 1 FROM originals WHERE team=NEW.team AND member=NEW.member) AND (SELECT rows>=16384 OR bytes+length(CAST(NEW.data AS BLOB))>67108864 FROM launch_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'member launch budget exceeded'); END;
CREATE TRIGGER launch_update_guard BEFORE UPDATE OF data ON originals WHEN (SELECT bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB))>67108864 FROM launch_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'member launch budget exceeded'); END;
CREATE TRIGGER notice_insert AFTER INSERT ON budget_alerts BEGIN UPDATE notice_budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_update AFTER UPDATE OF data ON budget_alerts BEGIN UPDATE notice_budget SET bytes=bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_delete AFTER DELETE ON budget_alerts BEGIN UPDATE notice_budget SET rows=rows-1,bytes=bytes-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_insert_guard BEFORE INSERT ON budget_alerts WHEN NOT EXISTS(SELECT 1 FROM budget_alerts WHERE team=NEW.team AND member=NEW.member) AND (SELECT rows>=16384 OR bytes+length(CAST(NEW.data AS BLOB))>67108864 FROM notice_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'launch notification budget exceeded'); END;
CREATE TRIGGER notice_update_guard BEFORE UPDATE OF data ON budget_alerts WHEN (SELECT bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB))>67108864 FROM notice_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'launch notification budget exceeded'); END;
CREATE TRIGGER preserve_launch_health BEFORE DELETE ON launch_health BEGIN SELECT RAISE(ABORT,'launch health singleton is retained'); END;
DELETE FROM "sqlite_sequence";
INSERT INTO "sqlite_sequence" VALUES('journal',5);
COMMIT;
PRAGMA user_version=4;
