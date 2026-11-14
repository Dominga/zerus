CREATE TABLE launch_budget(id INTEGER PRIMARY KEY CHECK(id=1),rows INTEGER NOT NULL CHECK(rows BETWEEN 0 AND 16384),bytes INTEGER NOT NULL CHECK(bytes BETWEEN 0 AND 67108864));
INSERT INTO launch_budget SELECT 1,COUNT(*),COALESCE(SUM(length(CAST(data AS BLOB))),0) FROM originals;
CREATE TRIGGER launch_insert AFTER INSERT ON originals BEGIN UPDATE launch_budget SET rows=rows+1,bytes=bytes+length(CAST(NEW.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER launch_update AFTER UPDATE OF data ON originals BEGIN UPDATE launch_budget SET bytes=bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE TRIGGER launch_delete AFTER DELETE ON originals BEGIN UPDATE launch_budget SET rows=rows-1,bytes=bytes-length(CAST(OLD.data AS BLOB)) WHERE id=1; END;
CREATE INDEX launch_settled ON originals(team,json_extract(data,'$.state'));
CREATE TRIGGER launch_journal_bound AFTER INSERT ON journal BEGIN DELETE FROM journal WHERE seq <= NEW.seq-10000; END;
CREATE TRIGGER launch_insert_guard BEFORE INSERT ON originals WHEN NOT EXISTS(SELECT 1 FROM originals WHERE team=NEW.team AND member=NEW.member) AND (SELECT rows>=16384 OR bytes+length(CAST(NEW.data AS BLOB))>67108864 FROM launch_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'member launch budget exceeded'); END;
CREATE TRIGGER launch_update_guard BEFORE UPDATE OF data ON originals WHEN (SELECT bytes+length(CAST(NEW.data AS BLOB))-length(CAST(OLD.data AS BLOB))>67108864 FROM launch_budget WHERE id=1) BEGIN SELECT RAISE(ABORT,'member launch budget exceeded'); END;
