-- Layout 9 of the timers extension, a step from layout 8: the lead's
-- answers reach their members.
-- A lead's answer reaches its member through the member's own read and a
-- doorbell. One mark per member: the newest reply to a question of the
-- attempt it holds, and how many replies of that attempt there were.
CREATE TABLE IF NOT EXISTS answer_marks (
 team TEXT NOT NULL, member TEXT NOT NULL, node INTEGER NOT NULL, generation INTEGER NOT NULL,
 revision INTEGER NOT NULL, replies INTEGER NOT NULL, PRIMARY KEY(team,member)
);
CREATE INDEX IF NOT EXISTS answer_marks_node ON answer_marks(team,node);
-- The questions of one attempt, newest first, without the team's history.
CREATE INDEX IF NOT EXISTS inbox_questions ON inbox(team,node,generation,seq) WHERE kind GLOB 'question:*';
-- An accept, a return, a reassignment, a cancel or a stop ends what a mark
-- points at: any decision on the node drops it.
CREATE TRIGGER IF NOT EXISTS answer_mark_drop_v1 AFTER INSERT ON decisions BEGIN
 DELETE FROM answer_marks WHERE team=NEW.team AND node=NEW.node;
END;
