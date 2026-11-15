CREATE TABLE IF NOT EXISTS non_delivery_progress (
 team TEXT PRIMARY KEY REFERENCES teams(id), after_seq INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS requests_effect_outcome ON requests (
 team, CASE WHEN json_valid(content) THEN json_extract(content,'$.effect_request_id') END);
