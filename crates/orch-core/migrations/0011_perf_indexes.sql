-- Schema version 11: performance indexes for in-flight polling and task deliveries.
-- Accelerates latest_delivery() lookups per milestone on every manager cycle, and lead queue deduplication.

CREATE INDEX IF NOT EXISTS deliveries_task_attempt_kind ON deliveries(task_id, attempt, kind, id);
CREATE INDEX IF NOT EXISTS lead_queue_unread_lookup ON lead_queue(read_at, kind, task_id);
