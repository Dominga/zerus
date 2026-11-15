CREATE TABLE IF NOT EXISTS planning_startup (
 team TEXT PRIMARY KEY REFERENCES teams(id), paused INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS teams_planning_live ON teams(id)
 WHERE status IN ('"planning"','"running"','"paused"');
