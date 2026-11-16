CREATE TABLE phase_reworks (
 seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL UNIQUE,
 team TEXT NOT NULL, phase INTEGER NOT NULL,
 base_publication TEXT NOT NULL REFERENCES phase_publications(id),
 comment TEXT NOT NULL, actor TEXT NOT NULL, created_ms INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('drafting','review','rejected','publishing','published')),
 generation INTEGER, header TEXT, approval TEXT REFERENCES phase_publications(id),
 FOREIGN KEY(team,phase) REFERENCES phases(team,n),
 CHECK((state='drafting')=(generation IS NULL)),
 CHECK((generation IS NULL)=(header IS NULL)),
 CHECK((state IN ('publishing','published'))=(approval IS NOT NULL))
);
CREATE INDEX phase_rework_current ON phase_reworks(team,phase,seq DESC);
CREATE UNIQUE INDEX phase_rework_open ON phase_reworks(team,phase) WHERE state<>'published';
CREATE TRIGGER phase_rework_identity BEFORE UPDATE OF id,team,phase,base_publication,comment,actor,created_ms ON phase_reworks
BEGIN SELECT RAISE(ABORT,'the original rework identity and reason are immutable'); END;
