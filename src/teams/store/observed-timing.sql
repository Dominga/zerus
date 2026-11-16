-- Optional display companions: no core/timer mark or execution requirement.
CREATE TABLE IF NOT EXISTS observed_timing_stream (
 id INTEGER PRIMARY KEY CHECK(id=1), identity TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS observed_timing_facts (
 seq INTEGER PRIMARY KEY, unit INTEGER NOT NULL REFERENCES observed_timing_units(id), team TEXT NOT NULL REFERENCES teams(id),
 phase INTEGER NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 effect TEXT, original_request TEXT, observed_ms INTEGER NOT NULL, anchor_ms INTEGER, last_verified_ms INTEGER, verified INTEGER NOT NULL,
 body TEXT NOT NULL CHECK(length(CAST(body AS BLOB))<=8192)
);
CREATE INDEX IF NOT EXISTS observed_timing_page ON observed_timing_facts(team,phase,seq);
CREATE INDEX IF NOT EXISTS observed_timing_unit ON observed_timing_facts(team,node,seq DESC);
CREATE INDEX IF NOT EXISTS observed_timing_effect ON observed_timing_facts(team,effect,seq);
CREATE TRIGGER IF NOT EXISTS observed_timing_kept BEFORE DELETE ON observed_timing_facts
 BEGIN SELECT RAISE(ABORT,'observations are append-only'); END;
CREATE TRIGGER IF NOT EXISTS observed_timing_immutable BEFORE UPDATE ON observed_timing_facts
 BEGIN SELECT RAISE(ABORT,'observations are immutable'); END;
CREATE TRIGGER IF NOT EXISTS observed_timing_stream_kept BEFORE DELETE ON observed_timing_stream
 BEGIN SELECT RAISE(ABORT,'observation source is retained'); END;
CREATE TRIGGER IF NOT EXISTS observed_timing_stream_immutable BEFORE UPDATE ON observed_timing_stream
 BEGIN SELECT RAISE(ABORT,'observation source is immutable'); END;

CREATE TABLE IF NOT EXISTS observed_timing_units (
 id INTEGER PRIMARY KEY, team TEXT NOT NULL, phase INTEGER NOT NULL,
 node INTEGER NOT NULL, digest TEXT NOT NULL, key TEXT NOT NULL,
 UNIQUE(team,digest), CHECK(length(CAST(key AS BLOB))<=2048)
);
CREATE INDEX IF NOT EXISTS observed_timing_cohort ON observed_timing_units(team,phase,id);
CREATE INDEX IF NOT EXISTS observed_timing_cohort_node ON observed_timing_units(team,phase,node,id);
CREATE INDEX IF NOT EXISTS observed_timing_unit_page ON observed_timing_facts(team,phase,unit,seq);
CREATE INDEX IF NOT EXISTS observed_timing_verified ON observed_timing_facts(team,phase,unit,seq) WHERE verified;
CREATE TRIGGER IF NOT EXISTS observed_timing_units_kept BEFORE DELETE ON observed_timing_units
 BEGIN SELECT RAISE(ABORT,'observation cohort history is retained'); END;
CREATE TRIGGER IF NOT EXISTS observed_timing_units_immutable BEFORE UPDATE ON observed_timing_units
 BEGIN SELECT RAISE(ABORT,'observation cohort history is immutable'); END;
CREATE UNIQUE INDEX IF NOT EXISTS observed_original_once ON observed_timing_facts(team,original_request) WHERE original_request IS NOT NULL;
CREATE INDEX IF NOT EXISTS observed_task_latest ON observed_timing_facts(team,node,json_extract(body,'$.source.kind'),seq DESC);
