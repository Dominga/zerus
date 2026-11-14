CREATE INDEX IF NOT EXISTS teams_live ON teams(id)
 WHERE status IN ('"running"','"paused"');
CREATE INDEX IF NOT EXISTS cards_waiting ON cards(seq) WHERE state='open';
CREATE INDEX IF NOT EXISTS effects_attempt ON effects(team,node,generation,seq);
CREATE INDEX IF NOT EXISTS integration_approved ON cards(team,decision_seq)
 WHERE kind='integration' AND state='closed' AND decision='approve';
CREATE INDEX IF NOT EXISTS integration_card ON cards(decision_seq)
 WHERE kind='integration';
CREATE INDEX IF NOT EXISTS inbox_unstaffed_told ON inbox(team,node,seq)
 WHERE kind LIKE 'unstaffed:%' AND reply IS NULL;
