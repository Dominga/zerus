CREATE TABLE IF NOT EXISTS pipeline_runs (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL, stage INTEGER,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE TABLE IF NOT EXISTS stage_tasks (
 team TEXT NOT NULL, node INTEGER NOT NULL, target INTEGER NOT NULL,
 generation INTEGER NOT NULL, state TEXT NOT NULL DEFAULT 'wanted',
 request_id TEXT UNIQUE, outcome TEXT, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE INDEX IF NOT EXISTS stage_tasks_pending ON stage_tasks(team,node)
 WHERE state IN ('wanted','reserved');
CREATE TABLE IF NOT EXISTS verdicts (
 team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 actor TEXT NOT NULL, pass INTEGER NOT NULL, text TEXT NOT NULL,
 PRIMARY KEY(team,node,generation), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);

-- zerus-63rru.20: every member who implemented any round of a target, with
-- the round that recorded them. The record per target is bounded by the
-- team's member bound. An older build ignores this table and reads the
-- store exactly as before (the node rows are unchanged).
CREATE TABLE IF NOT EXISTS target_implementers (
 team TEXT NOT NULL, target INTEGER NOT NULL, member TEXT NOT NULL, round INTEGER NOT NULL,
 PRIMARY KEY(team,target,member), FOREIGN KEY(team,target) REFERENCES node_identity(team,id)
);
