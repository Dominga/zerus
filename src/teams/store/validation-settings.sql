CREATE TABLE IF NOT EXISTS validation_settings (
    team TEXT PRIMARY KEY REFERENCES teams(id),
    data TEXT NOT NULL
);
