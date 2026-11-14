-- A singleton of integer fields; neither launch nor notice budgets charge it.
CREATE TABLE launch_health (
    id INTEGER PRIMARY KEY CHECK(id=1),
    code INTEGER CHECK(code IS NULL OR (typeof(code)='integer' AND code IN (1,2))),
    at_ms INTEGER CHECK(at_ms IS NULL OR (typeof(at_ms)='integer' AND at_ms>=0)),
    CHECK((code IS NULL)=(at_ms IS NULL))
);
INSERT INTO launch_health VALUES(1,NULL,NULL);
CREATE TRIGGER preserve_launch_health BEFORE DELETE ON launch_health BEGIN SELECT RAISE(ABORT,'launch health singleton is retained'); END;
