-- This package's layout. Every statement is idempotent: the transition in
-- timers/schema.rs runs them in one immediate transaction, triggers before
-- the backfills, and writes timers_extension last.
CREATE TABLE IF NOT EXISTS timers_extension (id INTEGER PRIMARY KEY CHECK(id=1), version INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS key_clear_teams (team TEXT PRIMARY KEY, pending INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS key_clear_pending ON key_clear_teams(team) WHERE pending=1;
-- The work frontier: exactly the nodes whose task key still waits for a
-- final record. A released stage enters it in a team of any status ('stage');
-- a stopped or archived team's node enters it once its cancel settles
-- ('stop'). A final record removes the row, so no read walks settled history.
CREATE TABLE IF NOT EXISTS key_clear_work (
 team TEXT NOT NULL, node INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('stage','stop')),
 PRIMARY KEY(team,node)
);
CREATE INDEX IF NOT EXISTS key_clear_work_kind ON key_clear_work(team,kind,node);
CREATE TABLE IF NOT EXISTS timer_firings (
 team TEXT NOT NULL, kind TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 item INTEGER NOT NULL, target TEXT NOT NULL CHECK(target IN ('inbox','card')),
 seq INTEGER NOT NULL, subject INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,kind,node,generation,item)
);
CREATE INDEX IF NOT EXISTS timer_card_subject ON timer_firings(team,subject) WHERE target='card';
CREATE INDEX IF NOT EXISTS timer_card_node ON timer_firings(team,node) WHERE target='card';
CREATE TABLE IF NOT EXISTS effect_outcomes (
 team TEXT NOT NULL, request_id TEXT NOT NULL, at_ms INTEGER NOT NULL,
 PRIMARY KEY(team,request_id)
);
CREATE TABLE IF NOT EXISTS key_clears (
 team TEXT NOT NULL, node INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('cleared','refused')),
 reason TEXT NOT NULL, at_ms INTEGER NOT NULL,
 PRIMARY KEY(team,node), FOREIGN KEY(team,node) REFERENCES node_identity(team,id)
);
CREATE INDEX IF NOT EXISTS key_refusals ON key_clears(team,node) WHERE state='refused';
CREATE INDEX IF NOT EXISTS key_refusals_newest ON key_clears(team,at_ms,node) WHERE state='refused';
CREATE TABLE IF NOT EXISTS late_sessions (
 team TEXT NOT NULL, member TEXT NOT NULL, launch TEXT NOT NULL, machine TEXT NOT NULL,
 session_id TEXT NOT NULL, name TEXT NOT NULL, card INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,member,launch)
);
-- A terminal team's task keys are cleared by the service in pages.
CREATE TRIGGER IF NOT EXISTS key_clear_terminal_v1 AFTER UPDATE OF status ON teams
 WHEN NEW.status IN ('"stopped"','"archived"') BEGIN
 INSERT OR IGNORE INTO key_clear_teams(team,pending) VALUES(NEW.id,1);
END;
-- A decision on a reported node settles its report reminder card, as the
-- service's own CloseCard would: closed, stale, decided by its opener.
CREATE TRIGGER IF NOT EXISTS timer_cards_decided_v1 AFTER INSERT ON decisions BEGIN
 UPDATE cards SET state='closed',decision='stale',decided_actor=actor
  WHERE state='open' AND kind='report_waiting' AND seq IN
  (SELECT seq FROM timer_firings WHERE team=NEW.team AND node=NEW.node AND target='card');
END;
-- A closed inbox item settles every timer card about it.
CREATE TRIGGER IF NOT EXISTS timer_cards_item_v1 AFTER UPDATE OF state ON inbox
 WHEN OLD.state='open' AND NEW.state<>'open' BEGIN
 UPDATE cards SET state='closed',decision='stale',decided_actor=actor
  WHERE state='open' AND kind IN ('report_waiting','silent_lead') AND seq IN
  (SELECT seq FROM timer_firings WHERE team=NEW.team AND subject=NEW.seq AND target='card');
END;
-- A stage is released when it is archived and its creation is no longer
-- live, in either order; one with a key to clear enters the frontier, and a
-- terminal team that had left the pending set returns to it.
CREATE TRIGGER IF NOT EXISTS key_clear_stage_archived_v2 AFTER INSERT ON node_archive
 WHEN json_extract(NEW.data,'$.kind')='stage' BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT a.team,a.id,'stage' FROM node_archive a
  WHERE a.team=NEW.team AND a.id=NEW.id AND json_extract(a.data,'$.kind')='stage'
  AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.state IN ('wanted','reserved','created'))
  AND (EXISTS(SELECT 1 FROM node_identity i WHERE i.team=a.team AND i.id=a.id AND i.task_uuid<>'')
   OR EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.request_id IS NOT NULL))
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=a.team AND k.node=a.id);
 UPDATE key_clear_teams SET pending=1 WHERE team=NEW.team AND pending=0
  AND EXISTS(SELECT 1 FROM key_clear_work w WHERE w.team=NEW.team AND w.node=NEW.id);
END;
CREATE TRIGGER IF NOT EXISTS key_clear_stage_settled_v2 AFTER UPDATE OF state ON stage_tasks
 WHEN NEW.state NOT IN ('wanted','reserved','created') BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT a.team,a.id,'stage' FROM node_archive a
  WHERE a.team=NEW.team AND a.id=NEW.node AND json_extract(a.data,'$.kind')='stage'
  AND NOT EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.state IN ('wanted','reserved','created'))
  AND (EXISTS(SELECT 1 FROM node_identity i WHERE i.team=a.team AND i.id=a.id AND i.task_uuid<>'')
   OR EXISTS(SELECT 1 FROM stage_tasks s WHERE s.team=a.team AND s.node=a.id AND s.request_id IS NOT NULL))
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=a.team AND k.node=a.id);
 UPDATE key_clear_teams SET pending=1 WHERE team=NEW.team AND pending=0
  AND EXISTS(SELECT 1 FROM key_clear_work w WHERE w.team=NEW.team AND w.node=NEW.node);
END;
-- After a stop a node's key is cleared only once its cancel has settled,
-- cancelled or refused: then, with a task to name, it enters the frontier.
CREATE TRIGGER IF NOT EXISTS key_clear_cancel_settled_v1 AFTER UPDATE OF state ON decisions
 WHEN NEW.kind='cancel' AND OLD.state<>'settled' AND NEW.state='settled' BEGIN
 INSERT OR IGNORE INTO key_clear_work(team,node,kind) SELECT i.team,i.id,'stop' FROM node_identity i
  LEFT JOIN stage_tasks s ON s.team=i.team AND s.node=i.id
  WHERE i.team=NEW.team AND i.id=NEW.node
  AND EXISTS(SELECT 1 FROM teams t WHERE t.id=NEW.team AND t.status IN ('"stopped"','"archived"'))
  AND (i.task_uuid<>'' OR s.request_id IS NOT NULL)
  AND NOT EXISTS(SELECT 1 FROM task_observations o WHERE o.team=i.team AND o.node=i.id AND json_extract(o.data,'$.key') IS NOT NULL)
  AND NOT EXISTS(SELECT 1 FROM key_clears k WHERE k.team=i.team AND k.node=i.id);
END;
-- The service's answer for each cancel decision it carried out.
CREATE TABLE IF NOT EXISTS cancel_outcomes (
 decision INTEGER PRIMARY KEY, team TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('cancelled','refused','released')), reason TEXT NOT NULL, at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS cancel_refusals ON cancel_outcomes(team,node) WHERE state='refused';
CREATE INDEX IF NOT EXISTS cancel_node_newest ON cancel_outcomes(team,node,decision DESC);
-- A node's decisions of one attempt, without its settled generations. The
-- generation comes before the node so that a node-range read keeps using
-- the index of open decisions.
CREATE INDEX IF NOT EXISTS decisions_attempt ON decisions(team,generation,node);
