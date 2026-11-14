-- Schema version 5: the scheduler's per-milestone dispatch intent and the agent benches (§10).
-- Ready and assigned stay derived views; only the lead's explicit requests and the benches
-- are stored, so a crash or a re-sync never loses the scheduling rules.

-- What the lead asked for one milestone (`orch dispatch Mn [--to W] [--tier T]`, and the
-- event manager's `ready_ts` while it waits for the lead). No row means "no request".
CREATE TABLE dispatch_intent (
    task_id  TEXT PRIMARY KEY REFERENCES tasks(id),
    go       INTEGER NOT NULL DEFAULT 0,   -- the lead said go (auto-dispatch after the wait uses it too)
    assign   TEXT,                         -- `--to`: this exact coder, for one dispatch
    min_tier TEXT,                         -- `--tier`: a minimum tier, as `small`/`medium`/`high`
    ready_ts INTEGER                        -- when it first stood ready for the lead
);

-- One bench (cooldown) per agent: rate limit, gone, or repeated failed prompts. A bench
-- expires by time; a `gone` bench also ends as soon as the agent is seen again (§15).
CREATE TABLE benches (
    agent  TEXT PRIMARY KEY,
    reason TEXT NOT NULL,                  -- 'rate-limit', 'gone', 'unreachable', ...
    until  INTEGER NOT NULL                -- epoch seconds the bench expires at
);
