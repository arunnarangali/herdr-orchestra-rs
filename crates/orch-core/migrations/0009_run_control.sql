-- Schema version 9: run-level controls and the audit trail of actions (§6B, §18).
-- `run_control` holds at most one row: what the developer asked of the event manager
-- (pause, resume, stop). No row means `running`. It lives apart from `run` so that re-syncing
-- the plan never clears it, and it survives a restart of the event manager.
CREATE TABLE run_control (
    id     INTEGER PRIMARY KEY CHECK (id = 1),
    state  TEXT NOT NULL CHECK (state IN ('running', 'paused', 'stopping', 'stopped')),
    since  INTEGER NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('cli', 'api', 'manager'))
);
-- Every developer/lead action (answer, dispatch, fix, approve, escalate, set, pause, resume,
-- stop) and where it came from. The task's transition history records the state change itself.
CREATE TABLE run_actions (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    ts      INTEGER NOT NULL,
    source  TEXT NOT NULL CHECK (source IN ('cli', 'api', 'manager')),
    action  TEXT NOT NULL,
    task_id TEXT,
    detail  TEXT NOT NULL DEFAULT ''
);
