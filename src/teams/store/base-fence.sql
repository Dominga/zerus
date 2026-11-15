-- Only advanced teams' hand-out/effect and integration rows require the new
-- connection capability. Check registration rather than call the function:
-- SQLite resolves function calls before a trigger's WHEN, which would also
-- reject an old connection's unrelated writes for unadvanced teams.
CREATE TRIGGER IF NOT EXISTS base_pins_effects_insert
BEFORE INSERT ON effects
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_effects_update
BEFORE UPDATE ON effects
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team OR team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_effects_delete
BEFORE DELETE ON effects
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_reports_insert
BEFORE INSERT ON reports
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_reports_update
BEFORE UPDATE ON reports
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team OR team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_reports_delete
BEFORE DELETE ON reports
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_integration_facts_insert
BEFORE INSERT ON integration_facts
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_integration_facts_update
BEFORE UPDATE ON integration_facts
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team OR team=NEW.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
CREATE TRIGGER IF NOT EXISTS base_pins_integration_facts_delete
BEFORE DELETE ON integration_facts
WHEN EXISTS(SELECT 1 FROM base_origins WHERE team=OLD.team)
 AND NOT EXISTS(SELECT 1 FROM pragma_function_list
                WHERE name='zerus_teams_base_pins_v13' AND narg=0 AND builtin=0)
BEGIN
 SELECT RAISE(ABORT,'This team''s base advanced; these rows require a newer Zerus writer.');
END;
