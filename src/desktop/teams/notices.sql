-- An independent bounded outbox still admits a team card when member rows are full.
CREATE TABLE budget_alerts (
    team TEXT NOT NULL,
    member TEXT NOT NULL CHECK(member='@budget'),
    version INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(team, member)
);
CREATE TABLE notice_budget(id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 16384),bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 67108864));
INSERT INTO notice_budget SELECT 1,COUNT(*),COALESCE(SUM(length(CAST(data AS BLOB))),0) FROM budget_alerts;
CREATE TRIGGER notice_insert AFTER INSERT ON budget_alerts BEGIN UPDATE notice_budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_update AFTER UPDATE OF data ON budget_alerts BEGIN UPDATE notice_budget SET bytes=bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_delete AFTER DELETE ON budget_alerts BEGIN UPDATE notice_budget SET rows=rows-1,bytes=bytes-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER notice_insert_guard BEFORE INSERT ON budget_alerts WHEN NOT EXISTS(SELECT 1 FROM budget_alerts WHERE team=NEW.team AND member=NEW.member) AND (SELECT rows>=16384 OR bytes+length(CAST(NEW.data AS BLOB))>67108864 FROM notice_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'launch notification budget exceeded'); END;
CREATE TRIGGER notice_update_guard BEFORE UPDATE OF data ON budget_alerts WHEN (SELECT bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB))>67108864 FROM notice_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'launch notification budget exceeded'); END;
