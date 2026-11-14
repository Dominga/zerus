CREATE TABLE IF NOT EXISTS phase_drafts (
 team TEXT NOT NULL, phase INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, updated_ms INTEGER NOT NULL,
 PRIMARY KEY(team,phase), FOREIGN KEY(team,phase) REFERENCES phases(team,n)
);
CREATE TABLE IF NOT EXISTS phase_draft_nodes (
 team TEXT NOT NULL, phase INTEGER NOT NULL, id INTEGER NOT NULL, data TEXT NOT NULL,
 PRIMARY KEY(team,id), FOREIGN KEY(team,phase) REFERENCES phase_drafts(team,phase)
);
CREATE INDEX IF NOT EXISTS phase_draft_page ON phase_draft_nodes(team,phase,id);
CREATE TABLE IF NOT EXISTS phase_draft_counters (
 team TEXT PRIMARY KEY REFERENCES teams(id), next_phase INTEGER NOT NULL
);
