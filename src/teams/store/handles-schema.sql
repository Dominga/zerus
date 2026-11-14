-- A launch-bound member's durable handle (zerus-63rru.53): the session ID of
-- the record its binding verified, written once. A row lasts only as long as
-- the membership: the triggers remove it when the member is removed or the
-- team finishes, stops or is archived, whichever program makes that change,
-- so that UNIQUE(machine,session_id) never keeps a session from another team.
CREATE TABLE IF NOT EXISTS member_handles (
 team TEXT NOT NULL, member TEXT NOT NULL, machine TEXT NOT NULL,
 session_id TEXT NOT NULL, launch TEXT NOT NULL, label_hint TEXT NOT NULL,
 created_ms INTEGER NOT NULL,
 PRIMARY KEY(team,member), UNIQUE(machine,session_id)
);
CREATE TRIGGER IF NOT EXISTS member_handles_terminal_v1 AFTER UPDATE OF status ON teams
 WHEN NEW.status IN ('"finished"','"stopped"','"archived"') BEGIN
 DELETE FROM member_handles WHERE team=NEW.id;
END;
CREATE TRIGGER IF NOT EXISTS member_handles_removed_v1 AFTER UPDATE OF state ON members
 WHEN NEW.state='"removed"' BEGIN
 DELETE FROM member_handles WHERE team=NEW.team AND member=NEW.id;
END;
