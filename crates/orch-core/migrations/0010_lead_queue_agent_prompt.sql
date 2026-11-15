-- Schema version 10: the blocked-agent prompt text on lead queue entries (§15A item 1).
-- When herdr reports an agent `blocked`, the queue entry records the agent's name and the
-- pane's visible prompt text (the command or question) next to the human-readable text, so
-- `orch events`, the REST API and the live stream can show who is waiting and for what.
-- Existing rows keep NULL in both columns; ALTER TABLE adds the columns in place.

ALTER TABLE lead_queue ADD COLUMN agent TEXT;
ALTER TABLE lead_queue ADD COLUMN prompt TEXT;
