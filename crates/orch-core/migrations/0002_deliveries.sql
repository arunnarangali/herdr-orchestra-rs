-- Schema version 2: pending prompt deliveries (§16) and the per-agent streak of
-- consecutive delivery failures (§15: 3 in a row bench the agent).
-- A delivery row is committed BEFORE anything is sent to the runtime; a crash between
-- "decided to send" and "sent" leaves an unconfirmed row that recovery either confirms
-- from the agent's status or re-sends to the SAME agent.

CREATE TABLE deliveries (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    agent         TEXT NOT NULL,
    task_id       TEXT NOT NULL REFERENCES tasks(id),
    attempt       INTEGER NOT NULL DEFAULT 0,
    kind          TEXT NOT NULL,
    prompt        TEXT NOT NULL,
    recorded_at   INTEGER NOT NULL,
    confirmed     INTEGER NOT NULL DEFAULT 0,
    confirmed_at  INTEGER,
    error_kind    TEXT,
    error_message TEXT
);
CREATE INDEX deliveries_unconfirmed ON deliveries(confirmed, id);
CREATE INDEX deliveries_agent ON deliveries(agent, id);

CREATE TABLE delivery_fails (
    agent TEXT PRIMARY KEY,
    fails INTEGER NOT NULL DEFAULT 0
);
