CREATE TABLE IF NOT EXISTS phase_reviews (
 team TEXT NOT NULL, phase INTEGER NOT NULL, generation INTEGER NOT NULL,
 rejection TEXT, comment TEXT, actor TEXT NOT NULL,
 PRIMARY KEY(team,phase), FOREIGN KEY(team,phase) REFERENCES phases(team,n)
);
CREATE TABLE IF NOT EXISTS phase_publications (
 id TEXT PRIMARY KEY, team TEXT NOT NULL, phase INTEGER NOT NULL,
 generation INTEGER NOT NULL, snapshot TEXT NOT NULL, initial INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('reserved','published')),
 created_ms INTEGER NOT NULL,
 UNIQUE(team,phase), FOREIGN KEY(team,phase) REFERENCES phases(team,n)
);
CREATE INDEX IF NOT EXISTS phase_publication_pending ON phase_publications(team,state,phase);
CREATE TABLE IF NOT EXISTS phase_publication_nodes (
 publication TEXT NOT NULL REFERENCES phase_publications(id),
 team TEXT NOT NULL, node INTEGER NOT NULL, task_uuid TEXT NOT NULL UNIQUE,
 request TEXT NOT NULL, state TEXT NOT NULL DEFAULT 'pending'
 CHECK(state IN ('pending','writing','created','refused','uncertain')),
 reason TEXT NOT NULL DEFAULT '', cleared INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(team,node), UNIQUE(publication,node)
);
CREATE INDEX IF NOT EXISTS phase_publication_work ON phase_publication_nodes(publication,state,node);
CREATE INDEX IF NOT EXISTS phase_nodes_live ON nodes(team,json_extract(data,'$.phase'),id);
CREATE INDEX IF NOT EXISTS phase_nodes_archive ON node_archive(team,json_extract(data,'$.phase'),id);
