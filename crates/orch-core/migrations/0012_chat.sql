-- Schema version 12: the developer <-> lead conversation (the GUI's chat).
-- Every message is stored BEFORE it is delivered; delivery is a state on the stored row, so a
-- restart of the event manager neither loses a message nor types it twice without saying so.
--   sender  who wrote it: the developer (through the API) or the lead (`orch say`)
--   kind    `message`, `feature_request` (carries `request`) or `plan_approval`
--   request JSON of the feature request's fields (Feature, Why, Must, ...), else NULL
--   about   the milestone a lead reply is about, else NULL
--   delivery  developer messages only: queued -> sent -> acknowledged, or failed; NULL for the lead's
--   attempts  how many times the event manager began typing it into the lead's pane
--   seq     bumped whenever the row is stored or changes; the live stream sends a `chat` event
--           for every row whose seq is newer than the last one it sent
CREATE TABLE chat_messages (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    ts            INTEGER NOT NULL,
    sender        TEXT NOT NULL CHECK (sender IN ('developer', 'lead')),
    kind          TEXT NOT NULL CHECK (kind IN ('message', 'feature_request', 'plan_approval')),
    text          TEXT NOT NULL,
    request       TEXT,
    about         TEXT,
    delivery      TEXT CHECK (delivery IS NULL OR delivery IN ('queued', 'sent', 'acknowledged', 'failed')),
    delivery_note TEXT,
    attempts      INTEGER NOT NULL DEFAULT 0,
    sent_at       INTEGER,
    seq           INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX chat_messages_seq ON chat_messages(seq);
CREATE INDEX chat_messages_delivery ON chat_messages(delivery, id);

-- One row: what the event manager last saw of the lead's pane (`idle`, `working`, `focused`
-- or `gone`), so the API can answer `lead_unavailable` without talking to herdr itself.
CREATE TABLE chat_lead (
    id      INTEGER PRIMARY KEY CHECK (id = 1),
    pane    TEXT NOT NULL,
    seen_at INTEGER NOT NULL
);
