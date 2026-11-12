-- Schema version 1: tasks, dependencies, transition history, events.
-- Later phases add tables (workers, sessions, deliveries, merge journal, lead queue…) as new
-- numbered migration files registered in `store::MIGRATIONS`.

CREATE TABLE tasks (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL DEFAULT '',
    status          TEXT NOT NULL DEFAULT 'pending',
    reason          TEXT NOT NULL DEFAULT '',
    worker          TEXT,
    last_worker     TEXT,
    model           TEXT,
    attempt         INTEGER NOT NULL DEFAULT 0,
    fix_rounds      INTEGER NOT NULL DEFAULT 0,
    review_rounds   INTEGER NOT NULL DEFAULT 0,
    conflict_rounds INTEGER NOT NULL DEFAULT 0,
    lost            INTEGER NOT NULL DEFAULT 0,
    escalated       INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE task_deps (
    task_id TEXT NOT NULL REFERENCES tasks(id),
    dep_id  TEXT NOT NULL,
    PRIMARY KEY (task_id, dep_id)
);

CREATE TABLE history (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    TEXT NOT NULL REFERENCES tasks(id),
    ts         INTEGER NOT NULL,
    from_state TEXT NOT NULL,
    to_state   TEXT NOT NULL,
    why        TEXT NOT NULL DEFAULT '',
    worker     TEXT,
    model      TEXT,
    attempt    INTEGER NOT NULL DEFAULT 0,
    forced     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX history_task ON history(task_id, id);

-- Same fields as Python's events.jsonl line (ts, task, from, to, why, agent, attempt, forced),
-- plus model and `lead_blocking` (the transition is one the lead must hear about).
CREATE TABLE events (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    ts            INTEGER NOT NULL,
    task_id       TEXT NOT NULL,
    from_state    TEXT NOT NULL,
    to_state      TEXT NOT NULL,
    why           TEXT NOT NULL DEFAULT '',
    agent         TEXT,
    model         TEXT,
    attempt       INTEGER NOT NULL DEFAULT 0,
    forced        INTEGER NOT NULL DEFAULT 0,
    lead_blocking INTEGER NOT NULL DEFAULT 0
);
