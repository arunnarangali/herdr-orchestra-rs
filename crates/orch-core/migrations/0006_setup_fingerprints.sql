-- Schema version 6: setup fingerprints (§16). One row per folder (a worker's worktree, or the
-- main folder) once the plan's setup command has run there: the fingerprint of the dependency
-- files at that moment. The command runs again only when the fingerprint changes. No row means
-- "never set up here".
CREATE TABLE setup_fingerprints (
    folder      TEXT PRIMARY KEY,   -- absolute path of the folder
    fingerprint TEXT NOT NULL,      -- 'none' when the folder has no dependency files
    updated_at  INTEGER NOT NULL
);
