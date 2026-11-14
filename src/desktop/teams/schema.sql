CREATE TABLE originals (
    team TEXT NOT NULL,
    member TEXT NOT NULL,
    version INTEGER NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY(team, member)
);
CREATE TABLE journal (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    team TEXT NOT NULL,
    member TEXT NOT NULL,
    version INTEGER NOT NULL,
    at_ms INTEGER NOT NULL,
    actor TEXT NOT NULL,
    state TEXT NOT NULL,
    UNIQUE(team, member, version)
);
CREATE INDEX journal_team_sequence ON journal(team, seq);
PRAGMA user_version=2;
