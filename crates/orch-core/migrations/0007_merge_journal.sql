-- Schema version 7: the merge journal and the landing problems (§13, §15, §16).

-- At most one row: the merge in progress. It is written BEFORE the first change to the feature
-- branch and removed in the same transaction that marks the milestone merged, so a crash at any
-- step leaves a row that the next start can finish or undo. The single row also means two merges
-- can never be journaled at once.
CREATE TABLE merge_journal (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    task_id     TEXT NOT NULL,
    step        TEXT NOT NULL,        -- started | squashed | checked | committed
    head_before TEXT NOT NULL,        -- the feature branch tip before the merge
    src         TEXT NOT NULL,        -- the milestone's saved work (a commit) being merged
    commit_sha  TEXT,                 -- the landed commit, once created
    started_at  INTEGER NOT NULL
);

-- Why a milestone could not land, kept for the coder's next job: a merge conflict (its files, and
-- for a milestone that was already approved, the saved work before the resolution) or a check that
-- fails on the feature branch (the tail of its output). One row per milestone; landing clears it.
CREATE TABLE merge_issues (
    task_id         TEXT PRIMARY KEY REFERENCES tasks(id),
    kind            TEXT NOT NULL CHECK (kind IN ('conflict', 'check')),
    files           TEXT NOT NULL DEFAULT '[]',   -- JSON array of conflicting files
    detail          TEXT NOT NULL DEFAULT '',     -- the check output tail
    resolution_base TEXT,                         -- commit to diff the resolution against
    rereview        INTEGER NOT NULL DEFAULT 0,   -- an approved milestone changed: review it again
    created_at      INTEGER NOT NULL
);
