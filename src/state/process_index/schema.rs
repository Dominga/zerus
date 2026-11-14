pub(super) const SCHEMA: &str = "
CREATE TABLE process_meta(id INTEGER PRIMARY KEY CHECK(id=1),schema INTEGER NOT NULL,scope TEXT NOT NULL,epoch TEXT NOT NULL,generation INTEGER NOT NULL,revision INTEGER NOT NULL,high_water INTEGER NOT NULL,complete INTEGER NOT NULL,scan_done INTEGER NOT NULL,scan_water INTEGER NOT NULL,cursor TEXT NOT NULL);
CREATE TABLE process_sources(source TEXT PRIMARY KEY,version INTEGER NOT NULL,last_seq INTEGER NOT NULL,publication TEXT NOT NULL);
CREATE TABLE process_anchors(pid INTEGER NOT NULL,birth TEXT NOT NULL,source TEXT NOT NULL,body TEXT NOT NULL);
CREATE UNIQUE INDEX process_anchors_pid ON process_anchors(pid);
CREATE INDEX process_anchors_source ON process_anchors(source);
CREATE TABLE process_edges(pid INTEGER NOT NULL,birth TEXT NOT NULL,edge_key TEXT NOT NULL,source TEXT NOT NULL,body TEXT NOT NULL);
CREATE UNIQUE INDEX process_edges_key ON process_edges(pid,birth,edge_key);
CREATE INDEX process_edges_source ON process_edges(source);
CREATE TABLE process_changes(seq INTEGER PRIMARY KEY,id TEXT NOT NULL UNIQUE,source TEXT NOT NULL,body TEXT NOT NULL,settled INTEGER NOT NULL);
CREATE UNIQUE INDEX process_pending_source ON process_changes(source) WHERE settled=0;
CREATE INDEX process_changes_page ON process_changes(seq);
";
pub(super) const ANCHOR: &str="SELECT substr(birth,1,257),substr(source,1,513),CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body ELSE NULL END FROM process_anchors INDEXED BY process_anchors_pid WHERE pid=?1 LIMIT 1";
pub(super) const EDGES: &str="SELECT substr(edge_key,1,1025),CASE WHEN length(CAST(body AS BLOB))<=4096 THEN body ELSE NULL END FROM process_edges INDEXED BY process_edges_key WHERE pid=?1 AND birth=?2 AND edge_key>?3 ORDER BY edge_key LIMIT ?4";
pub(super) const CHANGES: &str="SELECT seq,substr(id,1,129),substr(source,1,513),CASE WHEN length(CAST(body AS BLOB))<=16384 THEN body ELSE NULL END,settled FROM process_changes INDEXED BY process_changes_page WHERE seq>?1 ORDER BY seq LIMIT ?2";
