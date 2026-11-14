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
CREATE INDEX IF NOT EXISTS inbox_lead_queue ON inbox(team,seq)
 WHERE node=0 AND reply IS NULL AND (kind LIKE 'lead_admission:%' OR kind LIKE 'lead_routing:%');
CREATE INDEX IF NOT EXISTS nodes_lead_admission ON nodes(team,id)
 WHERE admitted=0 AND node_state IN ('waiting','pending');
CREATE INDEX IF NOT EXISTS nodes_lead_routing ON nodes(team,id)
 WHERE admitted=1 AND routed=0 AND node_state IN ('waiting','pending');

-- Dedicated partial indexes keep the list's capped probes independent of
-- timer question order and closed integration-approval lookups.
CREATE INDEX IF NOT EXISTS list_nodes_progress ON nodes(team,node_state,id)
 WHERE COALESCE(json_extract(data,'$.kind'),'task')='task';
CREATE INDEX IF NOT EXISTS list_archive_progress ON node_archive(team,node_state,id)
 WHERE COALESCE(json_extract(data,'$.kind'),'task')='task';
CREATE INDEX IF NOT EXISTS list_inbox_questions ON inbox(team,kind)
 WHERE state='open' AND kind GLOB 'question:*';
CREATE INDEX IF NOT EXISTS list_cards_blocked ON cards(team,kind) WHERE state='open';
CREATE INDEX IF NOT EXISTS list_phases_progress ON phases(team,json_extract(data,'$.state'));
CREATE INDEX IF NOT EXISTS list_members_progress ON members(team,state,id);
