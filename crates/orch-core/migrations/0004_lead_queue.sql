-- Schema version 4: the lead's event queue and its wake-up state (§9, §16).
-- A queue entry is written in the same transaction as the change that caused it (a task
-- transition, an answer, an escalation), so nothing is lost if the process dies. Reading the
-- entries with `orch events` is the acknowledgement (`read_at`).

CREATE TABLE lead_queue (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    ts      INTEGER NOT NULL,
    kind    TEXT NOT NULL,
    task_id TEXT,                       -- none for `ready`, `all_done`, `stopped`
    text    TEXT NOT NULL DEFAULT '',
    read_at INTEGER
);
CREATE INDEX lead_queue_unread ON lead_queue(read_at, id);

-- One row: where the confirmed wake-up delivery stands. Written BEFORE the prompt is typed,
-- so a crash between "send" and "acknowledge" neither loses the batch nor repeats it as new.
CREATE TABLE lead_wake (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    upto       INTEGER NOT NULL DEFAULT 0,  -- newest queue id the last wake-up covers
    since      INTEGER NOT NULL DEFAULT 0,  -- when the oldest entry of the batch was queued
    round_at   INTEGER NOT NULL DEFAULT 0,  -- when this round of wake-ups started
    sent_at    INTEGER NOT NULL DEFAULT 0,  -- when the last wake-up was typed
    sends      INTEGER NOT NULL DEFAULT 0,  -- rounds sent for this batch (ORCH_LEAD_MAX_WAKES)
    confirm_n  INTEGER NOT NULL DEFAULT 0,  -- quick re-wakes in this round (ORCH_LEAD_CONFIRM_WAKES)
    awaiting   INTEGER NOT NULL DEFAULT 0,  -- sent and not yet seen acknowledged
    notified   INTEGER NOT NULL DEFAULT 0,  -- the developer was told about this batch
    held       INTEGER NOT NULL DEFAULT 0,  -- the "held, pane busy" line was logged
    gone_told  INTEGER NOT NULL DEFAULT 0   -- the "lead is not in herdr" report was made
);

-- Milestones the lead (or the event manager, after its wait) has started: `undispatched`
-- is the ready milestones not listed here.
CREATE TABLE task_go (
    task_id TEXT PRIMARY KEY REFERENCES tasks(id),
    ts      INTEGER NOT NULL
);
