use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use jiff::civil::Date;
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::schema::MIGRATIONS;
use crate::domain::{invalid, required, Direction, Goal, GoalEdit, Measure, NewGoal, Status};
use crate::error::{GoalsError, Result};
use crate::pace::{self, Reading};
use crate::quantity::Quantity;
use crate::review::{last_activity, ReviewPolicy};

/// Columns read by [`map_goal`], in its order.
pub(super) const GOAL_COLUMNS: &str = "id, title, done_when, why, start_date, target_date,
     measure, start_value, target_value, unit, better, status, status_on,
     abandoned_reason, created_at, updated_at";

pub struct Database {
    conn: Mutex<Connection>,
}

/// A goal with its reading and review state on one day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub goal: Goal,
    pub reading: Reading,
    pub last_review: Option<Date>,
    pub review_due: bool,
    /// Nothing has happened for the policy's quiet period.
    pub quiet: bool,
}

impl Database {
    /// The per-user database: `<data dir>/goals/goals.db` (on Linux,
    /// `$XDG_DATA_HOME/goals/goals.db` or `~/.local/share/goals/goals.db`).
    pub fn default_path() -> Result<PathBuf> {
        dirs::data_dir()
            .map(|dir| dir.join("goals").join("goals.db"))
            .ok_or(GoalsError::NoDataDir)
    }

    /// Open (creating if needed) the per-user database. A failure is
    /// returned, never replaced by an empty store.
    pub fn open_default() -> Result<Self> {
        let path = Self::default_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        Self::open(&path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        Self::setup(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut conn)?;
        Ok(Database {
            conn: Mutex::new(conn),
        })
    }

    pub(super) fn conn(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (every
        // write is one statement or one transaction), so carry on.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn()
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    pub fn create_goal(&self, new: &NewGoal) -> Result<Goal> {
        new.validate()?;
        let id = new_id();
        let now = now();
        let m = MeasureColumns::of(&new.measure);
        self.conn().execute(
            "INSERT INTO goals (id, title, done_when, why, start_date, target_date,
                 measure, start_value, target_value, unit, better, status,
                 created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'active', ?12, ?12)",
            params![
                id,
                new.title.trim(),
                new.done_when.trim(),
                new.why.trim(),
                new.start_date.to_string(),
                new.target_date.map(|d| d.to_string()),
                m.kind,
                m.start,
                m.target,
                m.unit,
                m.better,
                now,
            ],
        )?;
        self.goal(&id)
    }

    pub fn goal(&self, id: &str) -> Result<Goal> {
        let conn = self.conn();
        goal_in(&conn, id)
    }

    /// Every goal: active ones first, then by target date (undated last),
    /// then by when they were set.
    pub fn goals(&self) -> Result<Vec<Goal>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(&format!(
            "SELECT {GOAL_COLUMNS} FROM goals
             ORDER BY status <> 'active', target_date IS NULL, target_date, created_at, id"
        ))?;
        let rows = stmt.query_map([], map_goal)?;
        rows.map(|r| r?).collect()
    }

    /// Replace a goal's editable fields. The measure's kind can change only
    /// while the goal has no milestones and no check-ins.
    pub fn edit_goal(&self, id: &str, edit: &GoalEdit) -> Result<Goal> {
        edit.validate()?;
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let old = goal_in(&tx, id)?;
        if old.measure.kind() != edit.measure.kind() {
            let recorded: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM milestones WHERE goal_id = ?1)
                     OR EXISTS (SELECT 1 FROM check_ins WHERE goal_id = ?1)",
                [id],
                |r| r.get(0),
            )?;
            if recorded {
                return Err(invalid(
                    "a goal's measure can't change kind once milestones or check-ins are recorded",
                ));
            }
        }
        let m = MeasureColumns::of(&edit.measure);
        tx.execute(
            "UPDATE goals SET title = ?2, done_when = ?3, why = ?4, start_date = ?5,
                 target_date = ?6, measure = ?7, start_value = ?8, target_value = ?9,
                 unit = ?10, better = ?11, updated_at = ?12
             WHERE id = ?1",
            params![
                id,
                edit.title.trim(),
                edit.done_when.trim(),
                edit.why.trim(),
                edit.start_date.to_string(),
                edit.target_date.map(|d| d.to_string()),
                m.kind,
                m.start,
                m.target,
                m.unit,
                m.better,
                now(),
            ],
        )?;
        let goal = goal_in(&tx, id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// Mark a goal achieved or abandoned (with a reason), or active again.
    pub fn set_status(&self, id: &str, status: &Status) -> Result<Goal> {
        let (name, on, reason) = match status {
            Status::Active => ("active", None, None),
            Status::Achieved { on } => ("achieved", Some(on.to_string()), None),
            Status::Abandoned { on, reason } => {
                required(reason, "say why the goal is abandoned")?;
                ("abandoned", Some(on.to_string()), Some(reason.trim()))
            }
        };
        let changed = self.conn().execute(
            "UPDATE goals SET status = ?2, status_on = ?3, abandoned_reason = ?4,
                 updated_at = ?5
             WHERE id = ?1",
            params![id, name, on, reason, now()],
        )?;
        if changed == 0 {
            return Err(not_found(id));
        }
        self.goal(id)
    }

    /// Delete a goal with its milestones, check-ins and reviews.
    pub fn delete_goal(&self, id: &str) -> Result<()> {
        if self
            .conn()
            .execute("DELETE FROM goals WHERE id = ?1", [id])?
            == 0
        {
            return Err(not_found(id));
        }
        Ok(())
    }

    /// A goal's reading and review state on `today`.
    pub fn summary(&self, id: &str, today: Date, policy: &ReviewPolicy) -> Result<Summary> {
        let goal = self.goal(id)?;
        self.summarise(goal, today, policy)
    }

    /// Summaries of every active goal, in [`Self::goals`]'s order.
    pub fn active_summaries(&self, today: Date, policy: &ReviewPolicy) -> Result<Vec<Summary>> {
        self.goals()?
            .into_iter()
            .filter(|g| g.status.is_active())
            .map(|g| self.summarise(g, today, policy))
            .collect()
    }

    fn summarise(&self, goal: Goal, today: Date, policy: &ReviewPolicy) -> Result<Summary> {
        let milestones = self.milestones(&goal.id)?;
        let check_ins = self.check_ins(&goal.id)?;
        let last_review = self.last_review(&goal.id, today)?;
        let last_check_in = check_ins.iter().map(|c| c.on).filter(|d| *d <= today).max();
        let reading = pace::read(&goal, &milestones, &check_ins, today);
        let activity = last_activity(&goal, last_review, last_check_in, today);
        Ok(Summary {
            review_due: policy.review_due(&goal, last_review, today),
            quiet: policy.quiet(&goal, activity, today),
            last_review,
            reading,
            goal,
        })
    }
}

pub(super) fn now() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

pub(super) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(super) fn not_found(id: &str) -> GoalsError {
    GoalsError::NotFound(id.to_owned())
}

pub(super) fn goal_in(conn: &Connection, id: &str) -> Result<Goal> {
    conn.query_row(
        &format!("SELECT {GOAL_COLUMNS} FROM goals WHERE id = ?1"),
        [id],
        map_goal,
    )
    .optional()?
    .ok_or_else(|| not_found(id))?
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let latest = MIGRATIONS.last().map_or(0, |(v, _)| *v);
    if version > latest {
        return Err(GoalsError::Damaged(format!(
            "the database is at schema version {version}, newer than this app's {latest}"
        )));
    }
    for (target, sql) in MIGRATIONS.iter().filter(|(v, _)| *v > version) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", target)?;
        tx.commit()?;
    }
    Ok(())
}

struct MeasureColumns<'a> {
    kind: &'static str,
    start: Option<i64>,
    target: Option<i64>,
    unit: Option<&'a str>,
    better: Option<&'static str>,
}

impl<'a> MeasureColumns<'a> {
    fn of(measure: &'a Measure) -> Self {
        let (start, target, unit, better) = match measure {
            Measure::Total {
                start,
                target,
                unit,
            } => (Some(*start), Some(*target), Some(unit.trim()), None),
            Measure::Level {
                start,
                target,
                unit,
                better,
            } => (
                Some(*start),
                Some(*target),
                Some(unit.trim()),
                Some(match better {
                    Direction::Up => "up",
                    Direction::Down => "down",
                }),
            ),
            Measure::Milestones | Measure::Done => (None, None, None, None),
        };
        MeasureColumns {
            kind: measure.kind(),
            start: start.map(Quantity::units),
            target: target.map(Quantity::units),
            unit,
            better,
        }
    }
}

pub(super) fn date(text: &str) -> Result<Date> {
    text.parse()
        .map_err(|_| GoalsError::Damaged(format!("{text:?} is not a date")))
}

pub(super) fn optional_date(text: Option<String>) -> Result<Option<Date>> {
    text.as_deref().map(date).transpose()
}

/// Map a row of [`GOAL_COLUMNS`]. The outer `Result` is SQLite's, the
/// inner one says whether the stored values make a goal.
fn map_goal(row: &Row) -> rusqlite::Result<Result<Goal>> {
    let id: String = row.get(0)?;
    let start_date: String = row.get(4)?;
    let target_date: Option<String> = row.get(5)?;
    let measure: String = row.get(6)?;
    let start_value: Option<i64> = row.get(7)?;
    let target_value: Option<i64> = row.get(8)?;
    let unit: Option<String> = row.get(9)?;
    let better: Option<String> = row.get(10)?;
    let status: String = row.get(11)?;
    let status_on: Option<String> = row.get(12)?;
    let reason: Option<String> = row.get(13)?;
    let title = row.get(1)?;
    let done_when = row.get(2)?;
    let why = row.get(3)?;
    let created_at = row.get(14)?;
    let updated_at = row.get(15)?;
    Ok((|| {
        let damaged = |what: &str| GoalsError::Damaged(format!("goal {id}: {what}"));
        let number = || -> Result<(Quantity, Quantity, String)> {
            match (start_value, target_value, unit.clone()) {
                (Some(s), Some(t), Some(u)) => {
                    Ok((Quantity::from_units(s), Quantity::from_units(t), u))
                }
                _ => Err(damaged("a number goal without its figures")),
            }
        };
        let measure = match measure.as_str() {
            "milestones" => Measure::Milestones,
            "done" => Measure::Done,
            "total" => {
                let (start, target, unit) = number()?;
                Measure::Total {
                    start,
                    target,
                    unit,
                }
            }
            "level" => {
                let (start, target, unit) = number()?;
                let better = match better.as_deref() {
                    Some("up") => Direction::Up,
                    Some("down") => Direction::Down,
                    _ => return Err(damaged("a level without a better direction")),
                };
                Measure::Level {
                    start,
                    target,
                    unit,
                    better,
                }
            }
            other => return Err(damaged(&format!("unknown measure {other:?}"))),
        };
        let on = || -> Result<Date> {
            date(
                status_on
                    .as_deref()
                    .ok_or_else(|| damaged("a status without its date"))?,
            )
        };
        let status = match status.as_str() {
            "active" => Status::Active,
            "achieved" => Status::Achieved { on: on()? },
            "abandoned" => Status::Abandoned {
                on: on()?,
                reason: reason
                    .clone()
                    .ok_or_else(|| damaged("abandoned without a reason"))?,
            },
            other => return Err(damaged(&format!("unknown status {other:?}"))),
        };
        Ok(Goal {
            start_date: date(&start_date)?,
            target_date: optional_date(target_date)?,
            id: id.clone(),
            title,
            done_when,
            why,
            measure,
            status,
            created_at,
            updated_at,
        })
    })())
}
