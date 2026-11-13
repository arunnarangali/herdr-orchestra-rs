-- Schema version 3: the validated run (`orch sync`). The plan's header settings, the team it
-- names, and each milestone's plan fields. Milestone progress (state, attempt, history) stays
-- in `tasks` / `history`, so re-syncing the plan never touches it.

-- One row: the run settings from the plan header.
CREATE TABLE run (
    id            INTEGER PRIMARY KEY CHECK (id = 1),
    branch        TEXT NOT NULL,
    base          TEXT,
    title         TEXT NOT NULL DEFAULT '',
    commits       TEXT NOT NULL DEFAULT 'per-milestone',
    isolation     TEXT NOT NULL DEFAULT 'worktree',
    max_parallel  INTEGER NOT NULL DEFAULT 2,
    setup_inputs  TEXT NOT NULL DEFAULT '[]',   -- JSON array of globs
    scope         TEXT NOT NULL DEFAULT 'warn',
    risk_hold     INTEGER NOT NULL DEFAULT 1,
    budget_s      INTEGER,
    check_command TEXT NOT NULL DEFAULT '',
    setup_command TEXT NOT NULL DEFAULT 'none',
    lead_mode     TEXT NOT NULL DEFAULT 'off',
    reviewer      TEXT,                          -- the reviewer's name, when there is one
    synced_at     INTEGER NOT NULL DEFAULT 0
);

-- The workers, the reviewer and the lead of the plan.
CREATE TABLE run_agents (
    role     TEXT NOT NULL CHECK (role IN ('worker', 'reviewer', 'lead')),
    name     TEXT NOT NULL,
    kind     TEXT NOT NULL,
    model    TEXT,
    tier     TEXT,
    detect   INTEGER NOT NULL DEFAULT 1,         -- does herdr watch this agent
    pane     TEXT,                               -- `@pane:<id>`
    position INTEGER NOT NULL,
    PRIMARY KEY (role, name)
);

-- The plan fields of each milestone, in plan order.
CREATE TABLE task_plan (
    task_id      TEXT PRIMARY KEY REFERENCES tasks(id),
    position     INTEGER NOT NULL,
    enabled      INTEGER NOT NULL DEFAULT 1,
    tier         TEXT,
    needs        TEXT NOT NULL DEFAULT '[]',     -- JSON array
    model_why    TEXT NOT NULL DEFAULT '',
    prefer       TEXT NOT NULL DEFAULT 'any',
    review       INTEGER NOT NULL DEFAULT 0,
    likely_files TEXT NOT NULL DEFAULT '[]',     -- JSON array
    parallel     INTEGER NOT NULL DEFAULT 0
);
