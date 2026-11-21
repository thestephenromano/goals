//! The database schema and its migrations, by `PRAGMA user_version`.
//!
//! Dates are ISO 8601 `YYYY-MM-DD` text, timestamps milliseconds since the
//! Unix epoch, amounts whole ten-thousandths (`Quantity`). IDs are UUIDs, so
//! records made on different devices never collide.

pub(crate) const SCHEMA_V1: &str = "
CREATE TABLE goals (
    id           TEXT PRIMARY KEY,
    title        TEXT NOT NULL,
    done_when    TEXT NOT NULL,
    why          TEXT NOT NULL DEFAULT '',
    start_date   TEXT NOT NULL,
    target_date  TEXT,
    measure      TEXT NOT NULL CHECK (measure IN ('milestones', 'total', 'level', 'done')),
    start_value  INTEGER,
    target_value INTEGER,
    unit         TEXT,
    better       TEXT CHECK (better IN ('up', 'down')),
    status       TEXT NOT NULL CHECK (status IN ('active', 'achieved', 'abandoned')),
    status_on    TEXT,
    abandoned_reason TEXT,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    CHECK ((measure IN ('total', 'level')) = (start_value IS NOT NULL AND target_value IS NOT NULL AND unit IS NOT NULL)),
    CHECK ((measure = 'level') = (better IS NOT NULL)),
    CHECK ((status = 'active') = (status_on IS NULL)),
    CHECK ((status = 'abandoned') = (abandoned_reason IS NOT NULL))
) STRICT;

CREATE TABLE milestones (
    id         TEXT PRIMARY KEY,
    goal_id    TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    title      TEXT NOT NULL,
    due        TEXT,
    sort_order REAL NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;
CREATE INDEX milestones_goal ON milestones(goal_id, sort_order);

CREATE TABLE check_ins (
    id           TEXT PRIMARY KEY,
    goal_id      TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    on_date      TEXT NOT NULL,
    note         TEXT NOT NULL DEFAULT '',
    value        INTEGER,
    -- Deleting a milestone keeps the check-in and its note.
    milestone_id TEXT REFERENCES milestones(id) ON DELETE SET NULL,
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL
) STRICT;
CREATE INDEX check_ins_goal ON check_ins(goal_id, on_date);
CREATE INDEX check_ins_milestone ON check_ins(milestone_id);

CREATE TABLE reviews (
    id         TEXT PRIMARY KEY,
    goal_id    TEXT NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    on_date    TEXT NOT NULL,
    committed  INTEGER NOT NULL CHECK (committed IN (0, 1)),
    on_track   INTEGER NOT NULL CHECK (on_track IN (0, 1)),
    next_step  TEXT NOT NULL DEFAULT '',
    created_at INTEGER NOT NULL
) STRICT;
CREATE INDEX reviews_goal ON reviews(goal_id, on_date);
";

/// Every migration, in order; entry `n` brings the database to version `n`.
pub(crate) const MIGRATIONS: &[(i64, &str)] = &[(1, SCHEMA_V1)];
