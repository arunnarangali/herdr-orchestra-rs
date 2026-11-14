-- Schema version 8: the run branch fails its check (§13, §15). At most one row: present while the
-- feature branch is known to be red after the final check, so autopilot re-checks it before it
-- carries on. It lives apart from `run` so that re-syncing the plan can never clear it.
CREATE TABLE integration_broken (
    id     INTEGER PRIMARY KEY CHECK (id = 1),
    reason TEXT NOT NULL,        -- what failed, e.g. "the final check"
    since  INTEGER NOT NULL
);
