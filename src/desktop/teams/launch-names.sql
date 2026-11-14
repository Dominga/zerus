-- One stable display slug per team on this computer. This is additive to
-- launch schema 4, like launch_checkout_objects; existing originals stay intact.
CREATE TABLE IF NOT EXISTS launch_team_names (
    team TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE CHECK(length(slug) BETWEEN 1 AND 48 AND slug NOT GLOB '*[^a-z0-9-]*')
);
